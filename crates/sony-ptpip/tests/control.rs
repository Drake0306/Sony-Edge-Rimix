use std::time::Duration;

use sony_ptpip::error::Error;
use sony_ptpip::ptp::codes::*;
use sony_ptpip::ptp::PtpValue;
use sony_ptpip::sony::{
    control, focus, read_changed_props, read_property, sdio_connect, set_value, shoot,
    toggle_record,
};
use sony_ptpip::{quirks_for, ConnectOptions, PtpEvent, PtpIpConnection, Quirks, WriteMode};
use sony_ptpip_sim::{spawn, SimCommand, SimConfig, SimHandle};
use tokio::sync::broadcast;

async fn start_with(config: SimConfig) -> (SimHandle, PtpIpConnection) {
    let sim = spawn(config).await;
    let opts = ConnectOptions {
        guid: [0x42; 16],
        name: "Rimix test".into(),
        ..ConnectOptions::default()
    };
    let (mut conn, _) = PtpIpConnection::connect(sim.addr, opts).await.unwrap();
    sdio_connect(&mut conn).await.unwrap();
    (sim, conn)
}

async fn start() -> (SimHandle, PtpIpConnection) {
    start_with(SimConfig::default()).await
}

async fn stop(sim: SimHandle, conn: PtpIpConnection) {
    conn.close().await.unwrap();
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

/// Waits up to 1 s for the next DevicePropChanged event naming `code`.
async fn wait_prop_changed(events: &mut broadcast::Receiver<PtpEvent>, code: u16) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let ev = events.recv().await.unwrap();
            if ev.code == EV_SONY_DEVICE_PROP_CHANGED && ev.params == vec![code as u32] {
                return;
            }
        }
    })
    .await
    .expect("DevicePropChanged within 1 s");
}

/// White balance colour temperature, a range-form property in the fixture.
const COLOR_TEMP: u16 = 0xD20F;

fn count_ops(sim: &SimHandle, op: u16) -> usize {
    let prefix = format!("op {op:#06x} ");
    sim.log
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.starts_with(&prefix))
        .count()
}

#[test]
fn zv_e10_and_unknown_models_write_directly() {
    let expected = Quirks {
        write_mode: WriteMode::Direct,
        live_view_via_http: false,
        pairing: true,
    };
    assert_eq!(quirks_for("ZV-E10"), expected);
    assert_eq!(quirks_for("Some Future Camera"), expected);
}

#[tokio::test]
async fn shoot_enqueues_one_capture() {
    let (sim, mut conn) = start().await;
    let mut events = conn.events();
    read_changed_props(&mut conn).await.unwrap();
    shoot(&mut conn, true).await.unwrap();
    wait_prop_changed(&mut events, PROP_CAPTURED_READY).await;
    let changed = read_changed_props(&mut conn).await.unwrap();
    let ready = changed
        .iter()
        .find(|p| p.code == PROP_CAPTURED_READY)
        .expect("0xD215 changed");
    assert_eq!(ready.current, PtpValue::U16(0x8001));
    stop(sim, conn).await;
}

#[tokio::test]
async fn shoot_without_af_enqueues_one_capture() {
    let (sim, mut conn) = start().await;
    shoot(&mut conn, false).await.unwrap();
    let ready = read_property(&mut conn, PROP_CAPTURED_READY).await.unwrap();
    assert_eq!(ready.current, PtpValue::U16(0x8001));
    assert_eq!(count_ops(&sim, OP_SDIO_CONTROL_DEVICE), 2);
    stop(sim, conn).await;
}

#[tokio::test]
async fn toggle_record_flips_state() {
    let (sim, mut conn) = start().await;
    let state = |p: sony_ptpip::sony::PropertyDesc| p.current;
    assert_eq!(
        state(
            read_property(&mut conn, PROP_MOVIE_REC_STATE)
                .await
                .unwrap()
        ),
        PtpValue::U8(0)
    );
    toggle_record(&mut conn).await.unwrap();
    assert_eq!(
        state(
            read_property(&mut conn, PROP_MOVIE_REC_STATE)
                .await
                .unwrap()
        ),
        PtpValue::U8(1)
    );
    toggle_record(&mut conn).await.unwrap();
    assert_eq!(
        state(
            read_property(&mut conn, PROP_MOVIE_REC_STATE)
                .await
                .unwrap()
        ),
        PtpValue::U8(0)
    );
    stop(sim, conn).await;
}

#[tokio::test]
async fn focus_sets_focus_found() {
    let (sim, mut conn) = start().await;
    let mut events = conn.events();
    // Holding the half press finds focus after the camera's 200 ms.
    control(&mut conn, CTRL_SHUTTER_HALF, &PtpValue::U16(2))
        .await
        .unwrap();
    wait_prop_changed(&mut events, PROP_FOCUS_FOUND).await;
    let found = read_property(&mut conn, PROP_FOCUS_FOUND).await.unwrap();
    assert_eq!(found.current, PtpValue::U8(1));
    control(&mut conn, CTRL_SHUTTER_HALF, &PtpValue::U16(1))
        .await
        .unwrap();
    wait_prop_changed(&mut events, PROP_FOCUS_FOUND).await;

    // focus() presses long enough to find focus, then lets go.
    focus(&mut conn).await.unwrap();
    wait_prop_changed(&mut events, PROP_FOCUS_FOUND).await;
    wait_prop_changed(&mut events, PROP_FOCUS_FOUND).await;
    let found = read_property(&mut conn, PROP_FOCUS_FOUND).await.unwrap();
    assert_eq!(found.current, PtpValue::U8(0));
    stop(sim, conn).await;
}

#[tokio::test]
async fn direct_set_value_changes_iso() {
    let (sim, mut conn) = start().await;
    let iso = read_property(&mut conn, PROP_ISO).await.unwrap();
    set_value(&mut conn, &iso, &PtpValue::U32(1600), &quirks_for("ZV-E10"))
        .await
        .unwrap();
    let iso = read_property(&mut conn, PROP_ISO).await.unwrap();
    assert_eq!(iso.current, PtpValue::U32(1600));
    assert_eq!(count_ops(&sim, OP_SDIO_CONTROL_DEVICE), 0);
    stop(sim, conn).await;
}

#[tokio::test]
async fn step_set_value_reaches_target_when_direct_writes_are_rejected() {
    let (sim, mut conn) = start_with(SimConfig {
        write_mode_step: true,
        ..SimConfig::default()
    })
    .await;
    let iso = read_property(&mut conn, PROP_ISO).await.unwrap();
    assert_eq!(iso.current, PtpValue::U32(800));
    set_value(&mut conn, &iso, &PtpValue::U32(1600), &quirks_for("ZV-E10"))
        .await
        .unwrap();
    let iso = read_property(&mut conn, PROP_ISO).await.unwrap();
    assert_eq!(iso.current, PtpValue::U32(1600));
    assert_eq!(count_ops(&sim, OP_SDIO_SET_EXT_DEVICE_PROP_VALUE), 1);
    // 800 → 1000 → 1250 → 1600 in the settable list.
    assert_eq!(count_ops(&sim, OP_SDIO_CONTROL_DEVICE), 3);
    stop(sim, conn).await;
}

#[tokio::test]
async fn step_mode_steps_down_without_trying_a_direct_write() {
    let (sim, mut conn) = start().await;
    let iso = read_property(&mut conn, PROP_ISO).await.unwrap();
    let quirks = Quirks {
        write_mode: WriteMode::Step,
        ..quirks_for("ZV-E10")
    };
    set_value(&mut conn, &iso, &PtpValue::U32(500), &quirks)
        .await
        .unwrap();
    let iso = read_property(&mut conn, PROP_ISO).await.unwrap();
    assert_eq!(iso.current, PtpValue::U32(500));
    assert_eq!(count_ops(&sim, OP_SDIO_SET_EXT_DEVICE_PROP_VALUE), 0);
    assert_eq!(count_ops(&sim, OP_SDIO_CONTROL_DEVICE), 2);
    stop(sim, conn).await;
}

#[tokio::test]
async fn set_value_rejects_target_not_in_list() {
    let (sim, mut conn) = start().await;
    let iso = read_property(&mut conn, PROP_ISO).await.unwrap();
    let err = set_value(&mut conn, &iso, &PtpValue::U32(1234), &quirks_for("ZV-E10"))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unsupported("value not allowed")));
    assert_eq!(count_ops(&sim, OP_SDIO_SET_EXT_DEVICE_PROP_VALUE), 0);
    assert_eq!(count_ops(&sim, OP_SDIO_CONTROL_DEVICE), 0);
    stop(sim, conn).await;
}

#[tokio::test]
async fn range_property_does_not_fall_back_to_steps() {
    let (sim, mut conn) = start_with(SimConfig {
        write_mode_step: true,
        ..SimConfig::default()
    })
    .await;
    let kelvin = read_property(&mut conn, COLOR_TEMP).await.unwrap();
    let err = set_value(
        &mut conn,
        &kelvin,
        &PtpValue::U16(5600),
        &quirks_for("ZV-E10"),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::Response { code: 0x201D, .. }));
    assert_eq!(count_ops(&sim, OP_SDIO_CONTROL_DEVICE), 0);
    stop(sim, conn).await;
}
