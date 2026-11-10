//! The Sony flows against the scripted adapter: no radio involved.

use rimix_ble::sony::{CHAR_PASSWORD, CHAR_REMOTE_COMMAND, CHAR_SSID, SHUTTER_SEQUENCE};
use rimix_ble::{handoff, shoot, BleAdapter, BleError, FakeAdapter, SonyAdvertisement};
use std::time::Duration;

const ZV: &str = "fake-zv-e10";

fn advert(name: &str) -> SonyAdvertisement {
    SonyAdvertisement {
        name: Some(name.to_string()),
        model_code: "E1".to_string(),
        pairing_open: true,
        address: ZV.to_string(),
    }
}

fn camera(ssid: &[u8], password: &[u8]) -> FakeAdapter {
    FakeAdapter::new()
        .device(ZV, advert("ZV-E10"))
        .characteristic(ZV, CHAR_SSID, ssid)
        .characteristic(ZV, CHAR_PASSWORD, password)
}

#[tokio::test]
async fn scan_lists_the_scripted_cameras() {
    let fake = camera(b"", b"");
    let found = fake.scan(Duration::from_millis(1)).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].address, ZV);
    assert_eq!(found[0].advert, advert("ZV-E10"));
}

#[tokio::test]
async fn handoff_reads_the_ssid_and_password() {
    let fake = camera(b"DIRECT-abc1:ZV-E10\0\0", b"12345678\0");
    let creds = handoff(&fake, ZV).await.unwrap();
    assert_eq!(creds.ssid, "DIRECT-abc1:ZV-E10");
    assert_eq!(creds.password, "12345678");
    assert!(fake.writes().is_empty(), "handoff must not write");
}

#[tokio::test]
async fn handoff_rejects_an_unknown_format_with_the_bytes_in_hex() {
    let fake = camera(&[0x01, 0x02, 0xAB], b"12345678");
    match handoff(&fake, ZV).await {
        Err(BleError::Io(m)) => {
            assert!(m.contains("unrecognised credential format"), "{m}");
            assert!(m.contains("0102ab"), "{m}");
            assert!(!m.contains("12345678"), "{m}");
        }
        other => panic!("expected Io, got {other:?}"),
    }
}

#[tokio::test]
async fn handoff_on_an_unknown_device_is_not_found() {
    let fake = camera(b"DIRECT-x", b"y");
    assert!(matches!(
        handoff(&fake, "elsewhere").await,
        Err(BleError::NotFound)
    ));
}

#[tokio::test]
async fn shoot_writes_the_four_frames_in_order() {
    let fake = camera(b"", b"");
    shoot(&fake, ZV).await.unwrap();
    let writes = fake.writes();
    assert_eq!(writes.len(), 4);
    for (write, frame) in writes.iter().zip(SHUTTER_SEQUENCE) {
        assert_eq!(write.0, ZV);
        assert_eq!(write.1, CHAR_REMOTE_COMMAND);
        assert_eq!(write.2, frame.to_vec());
    }
    assert_eq!(
        writes.iter().map(|w| w.2.clone()).collect::<Vec<_>>(),
        vec![
            vec![0x01, 0x07],
            vec![0x01, 0x09],
            vec![0x01, 0x08],
            vec![0x01, 0x06]
        ]
    );
}

#[tokio::test]
async fn the_dev_fake_is_a_zv_e10_with_pairing_open() {
    let fake = FakeAdapter::zv_e10();
    let found = fake.scan(Duration::from_millis(1)).await.unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0].advert.pairing_open);
    let creds = handoff(&fake, &found[0].address).await.unwrap();
    assert_eq!(creds.ssid, "DIRECT-abc1:ZV-E10");
    assert_eq!(creds.password, "12345678");
}

#[tokio::test]
async fn a_failed_full_press_still_lets_go_of_the_shutter() {
    let fake = camera(b"", b"").fail_write(1);
    assert!(matches!(shoot(&fake, ZV).await, Err(BleError::Io(_))));
    let frames: Vec<Vec<u8>> = fake.writes().into_iter().map(|w| w.2).collect();
    assert_eq!(
        frames,
        vec![vec![0x01, 0x07], vec![0x01, 0x08], vec![0x01, 0x06]]
    );
}

fn timed_out() -> BleError {
    BleError::Io("timed out after 5000 ms waiting to disconnect".into())
}

#[tokio::test]
async fn a_failed_disconnect_does_not_spoil_a_handoff() {
    let fake = camera(b"DIRECT-abc1:ZV-E10", b"12345678").fail_disconnect(timed_out());
    let creds = handoff(&fake, ZV).await.unwrap();
    assert_eq!(creds.ssid, "DIRECT-abc1:ZV-E10");
}

#[tokio::test]
async fn a_failed_disconnect_does_not_hide_the_real_error() {
    let fake = camera(b"", b"").fail_write(0).fail_disconnect(timed_out());
    assert_eq!(
        shoot(&fake, ZV).await,
        Err(BleError::Io("scripted write failure".into()))
    );
}
