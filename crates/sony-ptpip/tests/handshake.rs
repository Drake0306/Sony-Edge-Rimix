use sony_ptpip::error::Error;
use sony_ptpip::ptp::codes::*;
use sony_ptpip::sony::handshake::parse_ext_device_info;
use sony_ptpip::sony::sdio_connect;
use sony_ptpip::{ConnectOptions, PtpIpConnection};
use sony_ptpip_sim::{spawn, SimCommand, SimConfig};

fn opts() -> ConnectOptions {
    ConnectOptions {
        guid: [0x42; 16],
        name: "Rimix test".into(),
        ..ConnectOptions::default()
    }
}

#[test]
fn parses_ext_device_info_bytes() {
    let buf = [
        0x2C, 0x01, // version
        0x02, 0x00, 0x05, 0x50, 0x0D, 0xD2, // 2 properties
        0x01, 0x00, 0xC1, 0xD2, // 1 control
    ];
    let info = parse_ext_device_info(&buf).unwrap();
    assert_eq!(info.protocol_version, 0x012C);
    assert_eq!(info.properties, vec![0x5005, 0xD20D]);
    assert_eq!(info.controls, vec![0xD2C1]);
}

#[test]
fn rejects_truncated_ext_device_info() {
    let buf = [0x2C, 0x01, 0x02, 0x00, 0x05, 0x50];
    assert!(matches!(
        parse_ext_device_info(&buf),
        Err(Error::Protocol(_))
    ));
}

#[tokio::test]
async fn handshake_completes_against_sim() {
    let sim = spawn(SimConfig::default()).await;
    let (mut conn, _) = PtpIpConnection::connect(sim.addr, opts()).await.unwrap();
    let info = sdio_connect(&mut conn).await.unwrap();
    assert_eq!(info.protocol_version, 0x012C);
    assert!(info.properties.contains(&0xD20D));
    assert!(info.controls.contains(&0xD2C8));
    // Sony ops are no longer denied: a control without its data phase is
    // refused as a bad parameter, not with AccessDenied.
    let err = conn
        .command(OP_SDIO_CONTROL_DEVICE, &[CTRL_SHUTTER_HALF as u32])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Response { code: 0x201D, .. }));
    conn.close().await.unwrap();
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn sony_ops_before_handshake_are_denied() {
    let sim = spawn(SimConfig::default()).await;
    let (mut conn, _) = PtpIpConnection::connect(sim.addr, opts()).await.unwrap();
    let err = conn
        .command(OP_SDIO_GET_ALL_EXT_DEVICE_PROP_INFO, &[0])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Response { code: 0x200F, .. }));
    conn.close().await.unwrap();
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

async fn handshake_with(sdio_connect_data: bool) {
    let sim = spawn(SimConfig {
        sdio_connect_data,
        ..SimConfig::default()
    })
    .await;
    let (mut conn, _) = PtpIpConnection::connect(sim.addr, opts()).await.unwrap();
    let info = sdio_connect(&mut conn).await.unwrap();
    assert_eq!(info.protocol_version, 0x012C);
    // The stream is still aligned: the next transaction succeeds.
    let (blob, _) = conn
        .command_data_in(OP_SDIO_GET_ALL_EXT_DEVICE_PROP_INFO, &[0])
        .await
        .unwrap();
    assert!(!blob.is_empty());
    conn.close().await.unwrap();
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn handshake_accepts_data_from_sdio_connect() {
    // The ZV-E10 answers every SDIO_Connect phase with 8 bytes of data.
    handshake_with(true).await;
}

#[tokio::test]
async fn handshake_accepts_sdio_connect_without_data() {
    handshake_with(false).await;
}
