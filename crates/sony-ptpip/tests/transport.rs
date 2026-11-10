use std::time::Duration;

use sony_ptpip::error::Error;
use sony_ptpip::ptp::codes::*;
use sony_ptpip::ptp::device_info::parse_device_info;
use sony_ptpip::ptp::PtpValue;
use sony_ptpip::ptpip::Packet;
use sony_ptpip::{ConnectOptions, PtpIpConnection};
use sony_ptpip_sim::{spawn, SimCommand, SimConfig, SimHandle};

fn opts() -> ConnectOptions {
    ConnectOptions {
        guid: [0x42; 16],
        name: "Rimix test".into(),
        ..ConnectOptions::default()
    }
}

async fn start() -> (SimHandle, PtpIpConnection, Packet) {
    let sim = spawn(SimConfig::default()).await;
    let (conn, ack) = PtpIpConnection::connect(sim.addr, opts()).await.unwrap();
    (sim, conn, ack)
}

async fn shutdown(sim: SimHandle) {
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn connects_and_opens_session() {
    let (sim, conn, ack) = start().await;
    match ack {
        Packet::InitCommandAck { name, .. } => assert_eq!(name, "ZV-E10"),
        other => panic!("expected InitCommandAck, got {other:?}"),
    }
    assert!(sim
        .log
        .lock()
        .unwrap()
        .iter()
        .any(|l| l == "op 0x1002 tid 1"));
    conn.close().await.unwrap();
    shutdown(sim).await;
}

#[tokio::test]
async fn get_device_info_round_trip() {
    let (sim, mut conn, _) = start().await;
    let (data, params) = conn.command_data_in(OP_GET_DEVICE_INFO, &[]).await.unwrap();
    assert!(params.is_empty());
    let info = parse_device_info(&data).unwrap();
    assert_eq!(info.model, "ZV-E10");
    assert_eq!(info.vendor_ext_id, 0x11);
    assert_eq!(info.device_version, "2.00");
    assert!(info.operations.contains(&OP_SDIO_CONNECT));
    conn.close().await.unwrap();
    shutdown(sim).await;
}

#[tokio::test]
async fn unsupported_op_is_response_error() {
    let (sim, mut conn, _) = start().await;
    let err = conn.command(0x9999, &[]).await.unwrap_err();
    assert!(
        matches!(
            err,
            Error::Response {
                code: 0x2005,
                op: 0x9999
            }
        ),
        "got {err:?}"
    );
    conn.close().await.unwrap();
    shutdown(sim).await;
}

#[tokio::test]
async fn data_out_keeps_the_stream_in_step() {
    let (sim, mut conn, _) = start().await;
    let err = conn
        .command_data_out(OP_SDIO_SET_EXT_DEVICE_PROP_VALUE, &[0xD20D], &[1, 2, 3, 4])
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Response { code: 0x200F, .. }),
        "got {err:?}"
    );
    assert!(sim
        .log
        .lock()
        .unwrap()
        .iter()
        .any(|l| l == "op 0x9205 tid 2 data-out 4"));
    // The next transaction still lines up.
    conn.command_data_in(OP_GET_DEVICE_INFO, &[]).await.unwrap();
    conn.close().await.unwrap();
    shutdown(sim).await;
}

#[tokio::test]
async fn events_are_delivered() {
    let (sim, conn, _) = start().await;
    let mut rx = conn.events();
    sim.events
        .send(SimCommand::SetProperty {
            code: PROP_SHUTTER_SPEED,
            value: PtpValue::U32(0x0001_00FA),
        })
        .await
        .unwrap();
    let ev = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("event within 1 s")
        .unwrap();
    assert_eq!(ev.code, EV_SONY_DEVICE_PROP_CHANGED);
    assert_eq!(ev.params, vec![0xD20D]);
    conn.close().await.unwrap();
    shutdown(sim).await;
}

#[tokio::test]
async fn pairing_delay_is_tolerated() {
    let sim = spawn(SimConfig {
        require_pairing: true,
        pairing_delay: Duration::from_secs(2),
        ..SimConfig::default()
    })
    .await;
    let o = ConnectOptions {
        pairing_wait: Duration::from_secs(10),
        ..opts()
    };
    let (conn, _) = PtpIpConnection::connect(sim.addr, o).await.unwrap();
    conn.close().await.unwrap();
    shutdown(sim).await;
}

#[tokio::test]
async fn pairing_wait_expiry_is_pairing_timeout() {
    let sim = spawn(SimConfig {
        require_pairing: true,
        pairing_delay: Duration::from_secs(5),
        ..SimConfig::default()
    })
    .await;
    let o = ConnectOptions {
        pairing_wait: Duration::from_millis(200),
        ..opts()
    };
    let err = PtpIpConnection::connect(sim.addr, o).await.unwrap_err();
    assert!(matches!(err, Error::PairingTimeout), "got {err:?}");
    assert_eq!(
        err.to_string(),
        "the camera did not confirm the pairing in time"
    );
    shutdown(sim).await;
}

#[tokio::test]
async fn drop_connection_yields_disconnected() {
    let (sim, mut conn, _) = start().await;
    sim.events.send(SimCommand::DropConnection).await.unwrap();
    // The drop travels over a channel, not the socket; wait until it lands.
    tokio::time::timeout(Duration::from_secs(1), async {
        while !conn.is_disconnected() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("disconnect seen within 1 s");
    let err = conn
        .command_data_in(OP_GET_DEVICE_INFO, &[])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Disconnected), "got {err:?}");
    shutdown(sim).await;
}

#[tokio::test]
async fn timeout_poisons_the_connection() {
    let sim = spawn(SimConfig {
        delay_op: Some((OP_GET_DEVICE_INFO, Duration::from_millis(500))),
        ..SimConfig::default()
    })
    .await;
    let o = ConnectOptions {
        timeout: Duration::from_millis(200),
        ..opts()
    };
    let (mut conn, _) = PtpIpConnection::connect(sim.addr, o).await.unwrap();
    let err = conn
        .command_data_in(OP_GET_DEVICE_INFO, &[])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Timeout), "got {err:?}");
    assert!(conn.is_disconnected());
    let err = conn.command(0x9999, &[]).await.unwrap_err();
    assert!(matches!(err, Error::Disconnected), "got {err:?}");
    shutdown(sim).await;
}

#[tokio::test]
async fn response_error_does_not_poison() {
    let (sim, mut conn, _) = start().await;
    assert!(conn.command(0x9999, &[]).await.is_err());
    assert!(!conn.is_disconnected());
    conn.command_data_in(OP_GET_DEVICE_INFO, &[]).await.unwrap();
    conn.close().await.unwrap();
    shutdown(sim).await;
}
