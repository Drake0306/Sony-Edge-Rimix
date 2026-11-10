use std::path::Path;
use std::time::Duration;

use sony_ptpip::error::Error;
use sony_ptpip::ptp::codes::*;
use sony_ptpip::ptp::PtpValue;
use sony_ptpip::sony::{
    encode_all_props, parse_all_props, read_all_props, read_changed_props, read_property,
    sdio_connect, set_property,
};
use sony_ptpip::{ConnectOptions, PtpIpConnection};
use sony_ptpip_sim::{load_fixture, spawn, SimCommand, SimConfig, SimHandle};

async fn start() -> (SimHandle, PtpIpConnection) {
    let sim = spawn(SimConfig::default()).await;
    let opts = ConnectOptions {
        guid: [0x42; 16],
        name: "Rimix test".into(),
        ..ConnectOptions::default()
    };
    let (mut conn, _) = PtpIpConnection::connect(sim.addr, opts).await.unwrap();
    sdio_connect(&mut conn).await.unwrap();
    (sim, conn)
}

async fn stop(sim: SimHandle, conn: PtpIpConnection) {
    conn.close().await.unwrap();
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[test]
fn encode_then_parse_roundtrips_fixture() {
    let props = load_fixture(Path::new("fixtures/zv-e10.json")).unwrap();
    assert_eq!(props.len(), 16);
    assert_eq!(parse_all_props(&encode_all_props(&props)).unwrap(), props);
}

#[tokio::test]
async fn read_all_props_from_sim_has_shutter_1_125() {
    let (sim, mut conn) = start().await;
    let props = read_all_props(&mut conn).await.unwrap();
    let shutter = props.iter().find(|p| p.code == PROP_SHUTTER_SPEED).unwrap();
    assert_eq!(shutter.current, PtpValue::U32(0x0001_007D));
    assert_eq!(props.len(), 16);
    stop(sim, conn).await;
}

#[tokio::test]
async fn set_property_to_listed_value_changes_current_and_emits_event() {
    let (sim, mut conn) = start().await;
    let mut events = conn.events();
    // Start from a clean "changed" set.
    read_changed_props(&mut conn).await.unwrap();
    set_property(&mut conn, PROP_ISO, &PtpValue::U32(1600))
        .await
        .unwrap();
    let ev = tokio::time::timeout(Duration::from_secs(1), events.recv())
        .await
        .expect("event within 1 s")
        .unwrap();
    assert_eq!(ev.code, EV_SONY_DEVICE_PROP_CHANGED);
    assert_eq!(ev.params, vec![PROP_ISO as u32]);
    let changed = read_changed_props(&mut conn).await.unwrap();
    let iso = changed.iter().find(|p| p.code == PROP_ISO).unwrap();
    assert_eq!(iso.current, PtpValue::U32(1600));
    assert!(read_changed_props(&mut conn).await.unwrap().is_empty());
    stop(sim, conn).await;
}

#[tokio::test]
async fn set_property_to_unlisted_value_is_invalid_parameter() {
    let (sim, mut conn) = start().await;
    let err = set_property(&mut conn, PROP_ISO, &PtpValue::U32(1234))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Response { code: 0x201D, .. }));
    let iso = read_property(&mut conn, PROP_ISO).await.unwrap();
    assert_eq!(iso.current, PtpValue::U32(800));
    stop(sim, conn).await;
}

#[tokio::test]
async fn set_read_only_property_is_access_denied() {
    let (sim, mut conn) = start().await;
    let err = set_property(&mut conn, PROP_BATTERY, &PtpValue::U8(50))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Response { code: 0x200F, .. }));
    stop(sim, conn).await;
}

#[tokio::test]
async fn read_property_returns_one_entry() {
    let (sim, mut conn) = start().await;
    let wb = read_property(&mut conn, PROP_WHITE_BALANCE).await.unwrap();
    assert_eq!(wb.code, PROP_WHITE_BALANCE);
    assert_eq!(wb.current, PtpValue::U16(0x8007));
    let err = read_property(&mut conn, 0xD2FF).await.unwrap_err();
    assert!(matches!(err, Error::Response { code: 0x201D, .. }));
    stop(sim, conn).await;
}

#[tokio::test]
async fn sim_set_property_shows_in_changed_props() {
    let (sim, mut conn) = start().await;
    let mut events = conn.events();
    read_changed_props(&mut conn).await.unwrap();
    sim.events
        .send(SimCommand::SetProperty {
            code: PROP_F_NUMBER,
            value: PtpValue::U16(800),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), events.recv())
        .await
        .expect("event within 1 s")
        .unwrap();
    let changed = read_changed_props(&mut conn).await.unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].current, PtpValue::U16(800));
    stop(sim, conn).await;
}

#[tokio::test]
async fn ext_device_info_lists_fixture_properties() {
    let sim = spawn(SimConfig::default()).await;
    let opts = ConnectOptions {
        guid: [0x42; 16],
        name: "Rimix test".into(),
        ..ConnectOptions::default()
    };
    let (mut conn, _) = PtpIpConnection::connect(sim.addr, opts).await.unwrap();
    let info = sdio_connect(&mut conn).await.unwrap();
    assert_eq!(info.properties.len(), 16);
    assert!(info.properties.contains(&PROP_IMAGE_SIZE_TO_HOST));
    stop(sim, conn).await;
}
