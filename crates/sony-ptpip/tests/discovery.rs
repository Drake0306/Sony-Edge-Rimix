use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use sony_ptpip::discovery::{fetch_description, ssdp_search};
use sony_ptpip::{DiscoveredCamera, DiscoverySource};
use sony_ptpip_sim::{spawn, SimCommand, SimConfig, SimHandle, SshSimConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

async fn sim_with_ssdp() -> SimHandle {
    spawn(SimConfig {
        ssdp_port: Some(0),
        ..SimConfig::default()
    })
    .await
}

async fn stop(sim: SimHandle) {
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn ssdp_finds_the_simulator() {
    let sim = sim_with_ssdp().await;
    let ssdp_addr = sim.ssdp_addr.expect("simulator SSDP address");
    let http_addr = sim.http_addr.expect("simulator HTTP address");

    let locations = ssdp_search(Duration::from_millis(500), Some(ssdp_addr))
        .await
        .unwrap();
    assert_eq!(
        locations,
        vec![Url::parse(&format!("http://{http_addr}/dd.xml")).unwrap()]
    );

    let camera = fetch_description(&locations[0]).await.unwrap();
    assert_eq!(
        camera,
        DiscoveredCamera {
            model: "ZV-E10".into(),
            friendly_name: "ZV-E10".into(),
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            ptp_port: 15740,
            ptp_version: Some("3.00".into()),
            pairing_required: Some(true),
            ssh_supported: Some(false),
            source: DiscoverySource::Ssdp,
        }
    );
    stop(sim).await;
}

#[tokio::test]
async fn description_reports_ssh_when_the_simulator_has_an_ssh_front() {
    let sim = spawn(SimConfig {
        ssdp_port: Some(0),
        ssh: Some(SshSimConfig {
            user: "cam".into(),
            password: "12345678".into(),
        }),
        ..SimConfig::default()
    })
    .await;
    let http_addr = sim.http_addr.expect("simulator HTTP address");
    let location = Url::parse(&format!("http://{http_addr}/dd.xml")).unwrap();
    let camera = fetch_description(&location).await.unwrap();
    assert_eq!(camera.ssh_supported, Some(true));
    stop(sim).await;
}

#[tokio::test]
async fn ssdp_search_without_responders_returns_nothing() {
    // A bound but silent UDP socket stands in for a network with no cameras.
    let silent = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let target = silent.local_addr().unwrap();
    let locations = ssdp_search(Duration::from_millis(200), Some(target))
        .await
        .unwrap();
    assert!(locations.is_empty());
}

/// Serves `body` as text/xml to every request, for descriptions the
/// simulator does not provide.
async fn serve_once(body: &'static str) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 512];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut buf).await.unwrap();
            if n == 0 {
                return;
            }
            request.extend_from_slice(&buf[..n]);
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
    });
    addr
}

const DD_WITHOUT_IMAGING_DESC: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <friendlyName>Studio A7</friendlyName>
    <manufacturer>Sony Corporation</manufacturer>
    <modelName>ILCE-7M4</modelName>
  </device>
</root>"#;

#[tokio::test]
async fn fetch_description_handles_missing_imaging_desc() {
    let addr = serve_once(DD_WITHOUT_IMAGING_DESC).await;
    let location = Url::parse(&format!("http://{addr}/dd.xml")).unwrap();
    let camera = fetch_description(&location).await.unwrap();
    assert_eq!(camera.model, "ILCE-7M4");
    assert_eq!(camera.friendly_name, "Studio A7");
    assert_eq!(camera.ip, IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_eq!(camera.ptp_version, None);
    assert_eq!(camera.pairing_required, None);
    assert_eq!(camera.ssh_supported, None);
    assert_eq!(camera.source, DiscoverySource::Ssdp);
}

const DD_ROUTER: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <friendlyName>Home Router</friendlyName>
    <manufacturer>Acme Networks</manufacturer>
    <modelName>WR-9000</modelName>
  </device>
</root>"#;

#[tokio::test]
async fn fetch_description_rejects_devices_that_are_not_sony_cameras() {
    let addr = serve_once(DD_ROUTER).await;
    let location = Url::parse(&format!("http://{addr}/dd.xml")).unwrap();
    let err = fetch_description(&location).await.unwrap_err();
    assert!(
        matches!(err, sony_ptpip::error::Error::Unsupported(_)),
        "{err:?}"
    );
}
