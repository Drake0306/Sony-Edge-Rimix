use std::path::Path;
use std::time::Duration;

use chrono::NaiveDate;
use sony_ptpip::capture::{captured_ready, receive_one, Destination, DestinationMode};
use sony_ptpip::error::Error;
use sony_ptpip::ptp::codes::{
    EV_SONY_DEVICE_PROP_CHANGED, HANDLE_CAPTURED_STILL, OP_GET_OBJECT, PROP_CAPTURED_READY,
};
use sony_ptpip::sony::sdio_connect;
use sony_ptpip::{shoot, ConnectOptions, PtpIpConnection};
use sony_ptpip_sim::{spawn, SimCommand, SimConfig, SimHandle};

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

async fn stop(sim: SimHandle, conn: PtpIpConnection) {
    conn.close().await.unwrap();
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 10).unwrap()
}

fn custom(base: &Path) -> Destination {
    Destination {
        base: base.to_path_buf(),
        mode: DestinationMode::CustomFolder,
        recovery_dir: None,
    }
}

/// Names of the files in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn folder_per_date_creates_dated_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = Destination {
        base: tmp.path().to_path_buf(),
        mode: DestinationMode::FolderPerDate,
        recovery_dir: None,
    };
    let want = tmp.path().join("2026-11-10");
    assert_eq!(dest.target_dir(date()), want);
    assert_eq!(dest.ensure_writable(date()).unwrap(), want);
    assert!(want.is_dir());
}

#[test]
fn custom_folder_uses_base() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("shots");
    for mode in [
        DestinationMode::CustomFolder,
        DestinationMode::LightroomWatched,
    ] {
        let dest = Destination {
            base: base.clone(),
            mode,
            recovery_dir: None,
        };
        assert_eq!(dest.target_dir(date()), base);
        assert_eq!(dest.ensure_writable(date()).unwrap(), base);
        assert!(base.is_dir());
    }
}

#[test]
fn unwritable_destination_is_io_error() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("not-a-dir");
    std::fs::write(&file, b"x").unwrap();
    for mode in [
        DestinationMode::CustomFolder,
        DestinationMode::FolderPerDate,
    ] {
        let dest = Destination {
            base: file.clone(),
            mode,
            recovery_dir: None,
        };
        let err = dest.ensure_writable(date()).unwrap_err();
        assert!(matches!(err, Error::Io(_)), "{err:?}");
    }
}

#[tokio::test]
async fn receives_shot_into_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, mut conn) = start_with(SimConfig::default()).await;
    assert_eq!(captured_ready(&mut conn).await.unwrap(), 0);
    shoot(&mut conn, false).await.unwrap();
    assert_eq!(captured_ready(&mut conn).await.unwrap(), 1);

    let mut events = conn.events();
    let mut progress = Vec::new();
    let got = receive_one(&mut conn, &custom(tmp.path()), &mut |done, total| {
        progress.push((done, total))
    })
    .await
    .unwrap();

    assert_eq!(got.filename, "DSC00001.JPG");
    assert_eq!(got.camera_filename, "DSC00001.JPG");
    assert_eq!(got.path, tmp.path().join("DSC00001.JPG"));
    let bytes = std::fs::read(&got.path).unwrap();
    assert_eq!(bytes.len() as u64, got.bytes);
    assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).unwrap();
    assert_eq!((img.width(), img.height()), (640, 424));
    assert!(progress.len() >= 2, "{progress:?}");
    let &(done, total) = progress.last().unwrap();
    assert_eq!((done, total), (got.bytes, got.bytes));
    assert_eq!(names(tmp.path()), vec!["DSC00001.JPG"]);
    assert_eq!(captured_ready(&mut conn).await.unwrap(), 0);
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let ev = events.recv().await.unwrap();
            if ev.code == EV_SONY_DEVICE_PROP_CHANGED && ev.params == [PROP_CAPTURED_READY as u32] {
                return;
            }
        }
    })
    .await
    .expect("0xD215 change reported after the shot was fetched");
    stop(sim, conn).await;
}

#[tokio::test]
async fn existing_name_gets_suffix() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("DSC00001.JPG"), b"older").unwrap();
    std::fs::write(tmp.path().join("DSC00001-1.JPG"), b"older").unwrap();
    let (sim, mut conn) = start_with(SimConfig::default()).await;
    shoot(&mut conn, false).await.unwrap();
    let got = receive_one(&mut conn, &custom(tmp.path()), &mut |_, _| {})
        .await
        .unwrap();
    assert_eq!(got.filename, "DSC00001-2.JPG");
    assert_eq!(got.camera_filename, "DSC00001.JPG");
    assert_eq!(got.path, tmp.path().join("DSC00001-2.JPG"));
    assert_eq!(
        std::fs::read(tmp.path().join("DSC00001.JPG")).unwrap(),
        b"older"
    );
    assert_eq!(
        names(tmp.path()),
        vec!["DSC00001-1.JPG", "DSC00001-2.JPG", "DSC00001.JPG"]
    );
    stop(sim, conn).await;
}

#[tokio::test]
async fn nothing_waiting_is_a_response_error() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, mut conn) = start_with(SimConfig::default()).await;
    let err = receive_one(&mut conn, &custom(tmp.path()), &mut |_, _| {})
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Response { code: 0x2009, .. }),
        "{err:?}"
    );
    assert!(names(tmp.path()).is_empty());
    stop(sim, conn).await;
}

#[tokio::test]
async fn drop_mid_transfer_leaves_no_partial_file() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, mut conn) = start_with(SimConfig {
        drop_during_capture: true,
        ..SimConfig::default()
    })
    .await;
    shoot(&mut conn, false).await.unwrap();
    let err = receive_one(&mut conn, &custom(tmp.path()), &mut |_, _| {})
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Disconnected), "{err:?}");
    assert!(names(tmp.path()).is_empty(), "{:?}", names(tmp.path()));
    drop(conn);
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn write_failure_after_download_keeps_the_shot_in_the_recovery_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let shots = tmp.path().join("shots");
    std::fs::create_dir_all(&shots).unwrap();
    // Every name from DSC00001.JPG to DSC00001-999.JPG is taken, so the
    // download succeeds but the shot cannot be named in the target folder.
    std::fs::write(shots.join("DSC00001.JPG"), b"older").unwrap();
    for n in 1..=999 {
        std::fs::write(shots.join(format!("DSC00001-{n}.JPG")), b"older").unwrap();
    }
    let recovery = tmp.path().join("recovered");
    let dest = Destination {
        recovery_dir: Some(recovery.clone()),
        ..custom(&shots)
    };
    let (sim, mut conn) = start_with(SimConfig::default()).await;
    shoot(&mut conn, false).await.unwrap();
    let err = receive_one(&mut conn, &dest, &mut |_, _| {})
        .await
        .unwrap_err();
    let Error::Recovered { path, .. } = &err else {
        panic!("expected Recovered, got {err:?}");
    };
    assert_eq!(path, &recovery.join("DSC00001.JPG"));
    assert!(
        err.to_string().contains(&path.display().to_string()),
        "{err}"
    );
    let bytes = std::fs::read(path).unwrap();
    image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).unwrap();
    let left = names(&shots);
    assert_eq!(left.len(), 1000);
    assert!(left.iter().all(|n| !n.ends_with(".part")), "{left:?}");
    // The camera counts the shot as delivered.
    assert_eq!(captured_ready(&mut conn).await.unwrap(), 0);
    stop(sim, conn).await;
}

#[tokio::test]
async fn idle_limit_allows_a_slow_but_steady_transfer() {
    // Five packets 300 ms apart: about 1.2 s in all, never 1 s of silence.
    let (sim, mut conn) = start_with(SimConfig {
        capture_packet_pause: Some(Duration::from_millis(300)),
        ..SimConfig::default()
    })
    .await;
    shoot(&mut conn, false).await.unwrap();
    let started = std::time::Instant::now();
    let (data, _) = conn
        .command_data_in_idle(
            OP_GET_OBJECT,
            &[HANDLE_CAPTURED_STILL],
            Duration::from_secs(1),
        )
        .await
        .unwrap();
    assert!(started.elapsed() > Duration::from_secs(1));
    assert_eq!(&data[..2], &[0xFF, 0xD8]);
    stop(sim, conn).await;
}

#[tokio::test]
async fn idle_limit_times_out_when_the_camera_goes_quiet() {
    let (sim, mut conn) = start_with(SimConfig {
        capture_packet_pause: Some(Duration::from_millis(1500)),
        ..SimConfig::default()
    })
    .await;
    shoot(&mut conn, false).await.unwrap();
    let err = conn
        .command_data_in_idle(
            OP_GET_OBJECT,
            &[HANDLE_CAPTURED_STILL],
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Timeout), "{err:?}");
    assert!(conn.is_disconnected());
    drop(conn);
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn progress_is_reported_for_every_data_packet() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, mut conn) = start_with(SimConfig {
        capture_packet_pause: Some(Duration::from_millis(5)),
        ..SimConfig::default()
    })
    .await;
    shoot(&mut conn, false).await.unwrap();
    let mut progress = Vec::new();
    let got = receive_one(&mut conn, &custom(tmp.path()), &mut |done, total| {
        progress.push((done, total))
    })
    .await
    .unwrap();
    let received: Vec<u64> = progress.iter().map(|&(done, _)| done).collect();
    // The start, one call per packet (five), and the end.
    assert!(progress.len() >= 7, "{progress:?}");
    assert!(received.windows(2).all(|w| w[0] <= w[1]), "{progress:?}");
    assert!(
        received.iter().any(|&d| d > 0 && d < got.bytes),
        "{progress:?}"
    );
    assert!(progress.iter().all(|&(_, total)| total == got.bytes));
    stop(sim, conn).await;
}
