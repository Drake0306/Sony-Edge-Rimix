//! Send to Smartphone against the pretend camera: open, list, look, import, close.

use sony_ptpip::sendto::SendTo;
use sony_ptpip_sim::sendto::{original_jpeg, spawn_sendto, SendToSim};

#[tokio::test]
async fn the_pictures_chosen_on_the_camera_are_listed_looked_at_and_imported() {
    let sim = spawn_sendto(SendToSim::default()).await;
    let camera = SendTo::open(sim.addr).await.unwrap();
    assert_eq!(camera.model, "ZV-E10");
    assert_eq!(camera.root, "PushRoot");

    let top = camera.list(None).await.unwrap();
    assert_eq!(top.len(), 1);
    assert!(top[0].folder);
    assert_eq!(top[0].title, "2026-11-11");
    let files = camera.list(Some(&top[0].id)).await.unwrap();
    let names: Vec<&str> = files.iter().map(|f| f.title.as_str()).collect();
    assert_eq!(names, ["DSC00412.JPG", "DSC00413.ARW", "C0007.MP4"]);

    // A thumbnail and a larger picture to look at, without importing.
    let jpeg = &files[0];
    let thumb = camera.picture(jpeg.thumb.as_ref().unwrap()).await.unwrap();
    assert_eq!(&thumb[..2], &[0xFF, 0xD8]);
    let preview = camera
        .picture(jpeg.preview.as_ref().unwrap())
        .await
        .unwrap();
    assert_ne!(thumb, preview);

    // The import is the file as it was taken, with progress on the way.
    let dir = tempfile::tempdir().unwrap();
    let mut heard = Vec::new();
    let (path, bytes) = camera
        .download(
            jpeg.file.as_ref().unwrap(),
            dir.path(),
            &jpeg.title,
            |got, of| heard.push((got, of)),
        )
        .await
        .unwrap();
    assert_eq!(path, dir.path().join("DSC00412.JPG"));
    assert_eq!(std::fs::read(&path).unwrap(), original_jpeg());
    assert_eq!(bytes, original_jpeg().len() as u64);
    assert!(heard.len() > 1, "progress more than once: {}", heard.len());
    assert_eq!(heard.last().unwrap(), &(bytes, Some(bytes)));
    // A second import of the same file never replaces the first.
    let (again, _) = camera
        .download(
            jpeg.file.as_ref().unwrap(),
            dir.path(),
            &jpeg.title,
            |_, _| {},
        )
        .await
        .unwrap();
    assert_eq!(again, dir.path().join("DSC00412-1.JPG"));
    // Nothing half-written is left behind.
    let left: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("rimix-part"))
        .collect();
    assert!(left.is_empty(), "{left:?}");

    camera.close().await;
    let log = sim.log.lock().unwrap().join("\n");
    // The camera was told a transfer starts before anything was listed,
    // and that it is over at the end.
    let at = |what: &str| {
        log.find(what)
            .unwrap_or_else(|| panic!("no {what} in {log}"))
    };
    assert!(at("X_TransferStart") < at("Browse PushRoot 0"), "{log}");
    assert!(log.trim_end().ends_with("X_TransferEnd"), "{log}");
}

#[tokio::test]
async fn a_raw_the_camera_keeps_is_offered_as_its_preview_and_says_so_when_asked_for_itself() {
    let sim = spawn_sendto(SendToSim::default()).await;
    let camera = SendTo::open(sim.addr).await.unwrap();
    let top = camera.list(None).await.unwrap();
    let files = camera.list(Some(&top[0].id)).await.unwrap();
    let raw = &files[1];
    assert!(!raw.original);
    assert!(raw.file.as_ref().unwrap().contains("/LRG_"));
    let dir = tempfile::tempdir().unwrap();
    let original = raw.file.as_ref().unwrap().replace("/LRG_", "/ORG_");
    let err = camera
        .download(&original, dir.path(), &raw.title, |_, _| {})
        .await
        .unwrap_err();
    assert!(err.to_string().contains("406"), "{err}");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn a_camera_that_offers_everything_is_opened_at_its_other_root_and_long_folders_are_paged() {
    let sim = spawn_sendto(SendToSim {
        root: "PhotoRoot",
        page: 2,
    })
    .await;
    let camera = SendTo::open(sim.addr).await.unwrap();
    assert_eq!(camera.root, "PhotoRoot");
    let top = camera.list(None).await.unwrap();
    let files = camera.list(Some(&top[0].id)).await.unwrap();
    // Three files, two to a page.
    assert_eq!(files.len(), 3);
    let log = sim.log.lock().unwrap().join("\n");
    assert!(
        log.contains("Browse PhotoRoot_1 0") && log.contains("Browse PhotoRoot_1 2"),
        "{log}"
    );
}

#[tokio::test]
async fn nothing_listening_is_an_error_not_a_hang() {
    // A port nothing listens on.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    assert!(SendTo::open(addr).await.is_err());
}
