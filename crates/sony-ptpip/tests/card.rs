use std::path::Path;
use std::time::Duration;

use chrono::NaiveDate;
use sony_ptpip::capture::{Destination, DestinationMode};
use sony_ptpip::card::{download, list_objects, list_storages, CardKind, CARD_CHUNK};
use sony_ptpip::error::Error;
use sony_ptpip::ptp::codes::{
    FMT_JPEG, FMT_MP4, FMT_SONY_RAW, OP_GET_OBJECT_HANDLES, OP_GET_PARTIAL_OBJECT,
};
use sony_ptpip::sony::sdio_connect;
use sony_ptpip::{Camera, CameraConfig, CameraEvent, ConnectOptions, PtpIpConnection};
use sony_ptpip_sim::{spawn, SimCardFile, SimCommand, SimConfig, SimHandle};
use tokio::sync::broadcast;
use tokio::time::timeout;

const MIB: u64 = 1 << 20;
const WAIT: Duration = Duration::from_secs(20);

async fn connect(sim: &SimHandle) -> PtpIpConnection {
    let opts = ConnectOptions {
        guid: [0x42; 16],
        name: "Rimix test".into(),
        ..ConnectOptions::default()
    };
    let (mut conn, _) = PtpIpConnection::connect(sim.addr, opts).await.unwrap();
    sdio_connect(&mut conn).await.unwrap();
    conn
}

/// The simulator's byte `i` of the object `handle`.
fn sim_byte(handle: u32, i: u64) -> u8 {
    (i.wrapping_mul(31).wrapping_add(u64::from(handle))) as u8
}

/// True when `path` holds exactly the simulator's bytes for `handle`.
fn matches_sim(path: &Path, handle: u32, bytes: u64) -> bool {
    let data = std::fs::read(path).unwrap();
    data.len() as u64 == bytes
        && data
            .iter()
            .enumerate()
            .all(|(i, b)| *b == sim_byte(handle, i as u64))
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Offsets of the GetPartialObject requests logged from line `from` on.
fn partial_offsets(sim: &SimHandle, from: usize) -> Vec<u64> {
    let prefix = format!("op {OP_GET_PARTIAL_OBJECT:#06x} ");
    sim.log.lock().unwrap()[from..]
        .iter()
        .filter(|l| l.starts_with(&prefix))
        .map(|l| {
            let offset = l.split_whitespace().find_map(|w| w.strip_prefix("offset="));
            offset.unwrap().parse().unwrap()
        })
        .collect()
}

fn small_clip() -> Vec<SimCardFile> {
    vec![SimCardFile {
        folder: "PRIVATE/M4ROOT/CLIP".into(),
        filename: "C0001.MP4".into(),
        bytes: 4 * MIB,
        format: FMT_MP4,
        captured: NaiveDate::from_ymd_opt(2026, 11, 10)
            .unwrap()
            .and_hms_opt(10, 15, 0)
            .unwrap(),
    }]
}

#[tokio::test]
async fn list_objects_returns_the_card_with_kinds() {
    let sim = spawn(SimConfig::default()).await;
    let mut conn = connect(&sim).await;
    let storages = list_storages(&mut conn).await.unwrap();
    assert_eq!(storages.len(), 1);
    let objects = list_objects(&mut conn, storages[0]).await.unwrap();
    let summary: Vec<_> = objects
        .iter()
        .map(|o| (o.filename.as_str(), o.bytes, o.format, o.kind))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("DSC00010.JPG", 2 * MIB, FMT_JPEG, CardKind::Still),
            ("DSC00010.ARW", 24 * MIB, FMT_SONY_RAW, CardKind::Raw),
            ("C0001.MP4", 64 * MIB, FMT_MP4, CardKind::Video),
        ]
    );
    for o in &objects {
        assert_eq!(o.storage, storages[0]);
        assert!(o.captured.is_some(), "{o:?}");
        assert!(o.size_known, "{o:?}");
    }
    let mut handles: Vec<_> = objects.iter().map(|o| o.handle).collect();
    handles.sort();
    handles.dedup();
    assert_eq!(handles.len(), 3);
    conn.close().await.unwrap();
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

/// The `parent=` and `returned=` words of every GetObjectHandles logged.
fn handle_listings(sim: &SimHandle) -> Vec<(String, String)> {
    let prefix = format!("op {OP_GET_OBJECT_HANDLES:#06x} ");
    sim.log
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.starts_with(&prefix))
        .map(|l| {
            let word = |key: &str| {
                l.split_whitespace()
                    .find_map(|w| w.strip_prefix(key))
                    .unwrap_or_default()
                    .to_string()
            };
            (word("parent="), word("returned="))
        })
        .collect()
}

#[tokio::test]
async fn listing_walks_the_folders_from_a_root_of_folders() {
    let sim = spawn(SimConfig::default()).await;
    let mut conn = connect(&sim).await;
    let storage = list_storages(&mut conn).await.unwrap()[0];
    let objects = list_objects(&mut conn, storage).await.unwrap();
    assert_eq!(objects.len(), 3, "{objects:?}");
    let listings = handle_listings(&sim);
    assert_eq!(
        listings[0],
        ("0xffffffff".to_string(), "DCIM/,PRIVATE/".to_string()),
        "the root holds only folders"
    );
    let returned: Vec<&str> = listings.iter().map(|(_, r)| r.as_str()).collect();
    assert_eq!(
        returned,
        vec![
            "DCIM/,PRIVATE/",
            "100MSDCF/",
            "DSC00010.JPG,DSC00010.ARW",
            "M4ROOT/",
            "CLIP/",
            "C0001.MP4",
        ]
    );
    conn.close().await.unwrap();
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn an_object_without_info_is_skipped_not_fatal() {
    let sim = spawn(SimConfig {
        card_info_fails_for: Some("DSC00010.ARW".into()),
        ..SimConfig::default()
    })
    .await;
    let mut conn = connect(&sim).await;
    let storage = list_storages(&mut conn).await.unwrap()[0];
    let names: Vec<String> = list_objects(&mut conn, storage)
        .await
        .unwrap()
        .into_iter()
        .map(|o| o.filename)
        .collect();
    assert_eq!(names, vec!["DSC00010.JPG", "C0001.MP4"]);
    // The session is still usable afterwards.
    assert_eq!(list_objects(&mut conn, storage).await.unwrap().len(), 2);
    conn.close().await.unwrap();
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn downloading_the_clip_matches_the_simulators_bytes() {
    let sim = spawn(SimConfig::default()).await;
    let mut conn = connect(&sim).await;
    let storage = list_storages(&mut conn).await.unwrap()[0];
    let objects = list_objects(&mut conn, storage).await.unwrap();
    let clip = objects.iter().find(|o| o.kind == CardKind::Video).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let mut seen = Vec::new();
    let path = download(&mut conn, clip, tmp.path(), CARD_CHUNK, |got, total| {
        seen.push((got, total))
    })
    .await
    .unwrap();
    assert_eq!(path, tmp.path().join("C0001.MP4"));
    assert!(matches_sim(&path, clip.handle, clip.bytes));
    assert_eq!(names(tmp.path()), vec!["C0001.MP4"]);
    assert_eq!(seen.len(), 64);
    assert!(seen.windows(2).all(|w| w[0].0 < w[1].0), "{seen:?}");
    assert_eq!(seen.last(), Some(&(clip.bytes, clip.bytes)));
    let offsets = partial_offsets(&sim, 0);
    assert_eq!(offsets, (0..64).map(|n| n * MIB).collect::<Vec<_>>());
    conn.close().await.unwrap();
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn a_drop_mid_file_resumes_from_the_part_length() {
    let sim = spawn(SimConfig {
        card: small_clip(),
        drop_during_card_download_at: Some(1_500_000),
        ..SimConfig::default()
    })
    .await;
    let tmp = tempfile::tempdir().unwrap();
    let mut conn = connect(&sim).await;
    let storage = list_storages(&mut conn).await.unwrap()[0];
    let clip = list_objects(&mut conn, storage).await.unwrap().remove(0);
    // A leftover partial file of another file with the same name (another
    // camera, or before a card format) has a different size in its name:
    // it is removed, not resumed.
    let stale = tmp.path().join(".rimix-card-C0001.MP4.999.part");
    std::fs::write(&stale, vec![7u8; 100]).unwrap();

    let err = download(&mut conn, &clip, tmp.path(), CARD_CHUNK, |_, _| {})
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Disconnected | Error::Io(_)), "{err:?}");
    // Only the complete first chunk is kept, and nothing short is renamed
    // into place.
    assert!(!stale.exists());
    assert_eq!(partial_offsets(&sim, 0)[0], 0);
    assert_eq!(
        names(tmp.path()),
        vec![".rimix-card-C0001.MP4.4194304.part"]
    );
    let part = tmp.path().join(".rimix-card-C0001.MP4.4194304.part");
    assert_eq!(std::fs::metadata(&part).unwrap().len(), MIB);
    drop(conn);

    let mut conn = connect(&sim).await;
    let from = sim.log.lock().unwrap().len();
    let mut seen = Vec::new();
    let path = download(&mut conn, &clip, tmp.path(), CARD_CHUNK, |got, total| {
        seen.push((got, total))
    })
    .await
    .unwrap();
    assert_eq!(partial_offsets(&sim, from), vec![MIB, 2 * MIB, 3 * MIB]);
    assert_eq!(seen.first(), Some(&(2 * MIB, 4 * MIB)));
    assert!(matches_sim(&path, clip.handle, clip.bytes));
    assert_eq!(names(tmp.path()), vec!["C0001.MP4"]);
    conn.close().await.unwrap();
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn a_part_longer_than_the_file_is_discarded() {
    let sim = spawn(SimConfig {
        card: small_clip(),
        ..SimConfig::default()
    })
    .await;
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".rimix-card-C0001.MP4.4194304.part"),
        vec![0u8; 5 * MIB as usize],
    )
    .unwrap();
    let mut conn = connect(&sim).await;
    let storage = list_storages(&mut conn).await.unwrap()[0];
    let clip = list_objects(&mut conn, storage).await.unwrap().remove(0);
    let path = download(&mut conn, &clip, tmp.path(), CARD_CHUNK, |_, _| {})
        .await
        .unwrap();
    assert_eq!(partial_offsets(&sim, 0)[0], 0);
    assert!(matches_sim(&path, clip.handle, clip.bytes));
    assert_eq!(names(tmp.path()), vec!["C0001.MP4"]);
    conn.close().await.unwrap();
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

fn camera_config(sim: &SimHandle, dir: &Path) -> CameraConfig {
    CameraConfig {
        addr: sim.addr,
        guid: [0x42; 16],
        host_name: "Rimix on test".into(),
        destination: Destination {
            base: dir.to_path_buf(),
            mode: DestinationMode::CustomFolder,
            recovery_dir: None,
        },
        auto_receive: true,
        reconnect: true,
        before_reconnect: None,
        ssh: None,
    }
}

#[tokio::test]
async fn camera_lists_the_card_and_downloads_with_progress() {
    let sim = spawn(SimConfig::default()).await;
    let tmp = tempfile::tempdir().unwrap();
    // A clash with a file already there gets a -1 suffix.
    std::fs::write(tmp.path().join("DSC00010.JPG"), b"older").unwrap();
    let camera = Camera::connect(camera_config(&sim, tmp.path()))
        .await
        .unwrap();
    let card = camera.card_list().await.unwrap();
    assert_eq!(card.len(), 3);
    let jpeg = card.iter().find(|o| o.kind == CardKind::Still).unwrap();

    let mut rx = camera.subscribe();
    let path = camera.card_download(jpeg.handle).await.unwrap();
    assert_eq!(path, tmp.path().join("DSC00010-1.JPG"));
    assert!(matches_sim(&path, jpeg.handle, jpeg.bytes));

    let mut progress = Vec::new();
    let saved = timeout(WAIT, async {
        loop {
            match rx.recv().await {
                Ok(CameraEvent::CardDownloadProgress {
                    handle,
                    filename,
                    received,
                    total,
                }) => {
                    assert_eq!(handle, jpeg.handle);
                    assert_eq!(filename, "DSC00010.JPG");
                    assert_eq!(total, jpeg.bytes);
                    progress.push(received);
                }
                Ok(CameraEvent::CaptureSaved(r)) => return r,
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(e) => panic!("{e}"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(progress, vec![MIB, 2 * MIB]);
    assert_eq!(saved.path, path);
    assert_eq!(saved.filename, "DSC00010-1.JPG");
    assert_eq!(saved.camera_filename, "DSC00010.JPG");
    assert_eq!(saved.bytes, jpeg.bytes);
    camera.disconnect().await;
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[test]
fn card_download_progress_serialises() {
    let ev = CameraEvent::CardDownloadProgress {
        handle: 7,
        filename: "C0001.MP4".into(),
        received: 1,
        total: 2,
    };
    assert_eq!(
        serde_json::to_value(&ev).unwrap(),
        serde_json::json!({"CardDownloadProgress": {
            "handle": 7, "filename": "C0001.MP4", "received": 1, "total": 2
        }})
    );
}

#[tokio::test]
async fn files_of_4_gib_or_more_are_refused_before_any_part_file() {
    let mut card = small_clip();
    card[0].bytes = 5 * 1024 * MIB;
    let sim = spawn(SimConfig {
        card,
        ..SimConfig::default()
    })
    .await;
    let tmp = tempfile::tempdir().unwrap();
    let mut conn = connect(&sim).await;
    let storage = list_storages(&mut conn).await.unwrap()[0];
    let clip = list_objects(&mut conn, storage).await.unwrap().remove(0);
    assert!(!clip.size_known, "{clip:?}");
    let err = download(&mut conn, &clip, tmp.path(), CARD_CHUNK, |_, _| {})
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::Unsupported("files of 4 GiB or more cannot be downloaded yet")
        ),
        "{err:?}"
    );
    assert!(names(tmp.path()).is_empty(), "{:?}", names(tmp.path()));
    assert!(partial_offsets(&sim, 0).is_empty());
    conn.close().await.unwrap();
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}
