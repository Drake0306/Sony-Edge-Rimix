//! A unicast SSDP responder and a tiny HTTP server for the UPnP
//! descriptions, so discovery can be tested over loopback.

use std::net::SocketAddr;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::task::JoinSet;

fn dd_xml(http: SocketAddr) -> String {
    format!(
        r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0" xmlns:av="urn:schemas-sony-com:av">
  <specVersion>
    <major>1</major>
    <minor>0</minor>
  </specVersion>
  <device>
    <deviceType>urn:schemas-upnp-org:device:Basic:1</deviceType>
    <friendlyName>ZV-E10</friendlyName>
    <manufacturer>Sony Corporation</manufacturer>
    <modelName>ZV-E10</modelName>
    <UDN>uuid:rimix-sim</UDN>
    <av:X_DigitalImagingDescURL>http://{http}/DigitalImagingDesc.xml</av:X_DigitalImagingDescURL>
  </device>
</root>
"#
    )
}

fn digital_imaging_desc(ssh: bool) -> String {
    format!(
        r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-sony-com:av">
  <X_PTP_Information>
    <X_PTP_Versions>3.00</X_PTP_Versions>
    <X_PTP_PairingNecessity>true</X_PTP_PairingNecessity>
    <X_SSH_Support>{ssh}</X_SSH_Support>
  </X_PTP_Information>
</root>
"#
    )
}

/// Binds the SSDP responder on 127.0.0.1:`ssdp_port` and the HTTP server on
/// an OS-chosen port, spawning both into `tasks`. `ssh` is what the
/// description reports as `X_SSH_Support`.
///
/// Returns the SSDP and HTTP addresses.
pub(crate) async fn start(
    ssdp_port: u16,
    ssh: bool,
    tasks: &mut JoinSet<()>,
) -> (SocketAddr, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind simulator HTTP server");
    let http_addr = listener.local_addr().expect("HTTP server address");
    let socket = UdpSocket::bind(("127.0.0.1", ssdp_port))
        .await
        .expect("bind simulator SSDP responder");
    let ssdp_addr = socket.local_addr().expect("SSDP responder address");
    tasks.spawn(serve_http(listener, http_addr, ssh));
    tasks.spawn(answer_searches(socket, http_addr));
    (ssdp_addr, http_addr)
}

async fn answer_searches(socket: UdpSocket, http: SocketAddr) {
    let reply = format!(
        "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nEXT:\r\n\
         LOCATION: http://{http}/dd.xml\r\nSERVER: UPnP/1.0 Sony\r\n\
         ST: upnp:rootdevice\r\nUSN: uuid:rimix-sim::upnp:rootdevice\r\n\r\n"
    );
    let mut buf = [0u8; 2048];
    loop {
        let (n, peer) = match socket.recv_from(&mut buf).await {
            Ok(received) => received,
            Err(e) => {
                tracing::debug!(error = %e, "simulator SSDP receive failed");
                continue;
            }
        };
        if buf[..n].starts_with(b"M-SEARCH") {
            if let Err(e) = socket.send_to(reply.as_bytes(), peer).await {
                tracing::debug!(error = %e, "simulator SSDP reply failed");
            }
        }
    }
}

async fn serve_http(listener: TcpListener, http: SocketAddr, ssh: bool) {
    // Dropping the set when this task is aborted ends every connection.
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    connections.spawn(respond(stream, http, ssh));
                }
                Err(e) => tracing::warn!(error = %e, "simulator HTTP accept failed"),
            },
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

/// Answers one GET with a description, or 404.
async fn respond(stream: TcpStream, http: SocketAddr, ssh: bool) {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).await.is_err() {
        return;
    }
    let mut header = String::new();
    loop {
        header.clear();
        match reader.read_line(&mut header).await {
            Ok(0) | Err(_) => return,
            Ok(_) if header.trim_end().is_empty() => break,
            Ok(_) => {}
        }
    }
    let path = request_line.split_whitespace().nth(1).unwrap_or("");
    let body = match path {
        "/dd.xml" => Some(dd_xml(http)),
        "/DigitalImagingDesc.xml" => Some(digital_imaging_desc(ssh)),
        _ => None,
    };
    let response = match body {
        Some(body) => format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
        None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
    };
    let mut stream = reader.into_inner();
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}
