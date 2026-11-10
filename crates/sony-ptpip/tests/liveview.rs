use sony_ptpip::liveview::{next_frame, start};
use sony_ptpip::sony::sdio_connect;
use sony_ptpip::{ConnectOptions, PtpIpConnection};
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
    start(&mut conn).await.unwrap();
    (sim, conn)
}

async fn stop(sim: SimHandle, conn: PtpIpConnection) {
    conn.close().await.unwrap();
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn streams_ten_frames_from_sim() {
    let (sim, mut conn) = start_with(SimConfig::default()).await;
    let mut jpegs = Vec::new();
    for _ in 0..10 {
        let frame = next_frame(&mut conn).await.unwrap().expect("a frame");
        assert_eq!(&frame.jpeg[..2], &[0xFF, 0xD8]);
        let img = image::load_from_memory_with_format(&frame.jpeg, image::ImageFormat::Jpeg)
            .expect("frame decodes as JPEG");
        assert_eq!((img.width(), img.height()), (640, 424));
        assert_eq!(frame.level_deg, Some(1.2));
        assert_eq!(frame.focus_frames.len(), 1);
        assert_eq!(frame.focus_frames[0].kind, 2);
        assert_eq!(frame.focus_frames[0].cx, 0.5);
        jpegs.push(frame.jpeg);
    }
    // The picture moves from frame to frame.
    assert_ne!(jpegs[0], jpegs[1]);
    stop(sim, conn).await;
}

#[tokio::test]
async fn access_denied_frames_are_skipped_not_errors() {
    let (sim, mut conn) = start_with(SimConfig {
        live_view_busy_every: Some(3),
        ..SimConfig::default()
    })
    .await;
    let mut some = 0;
    let mut none = 0;
    for _ in 0..9 {
        match next_frame(&mut conn).await.unwrap() {
            Some(_) => some += 1,
            None => none += 1,
        }
    }
    assert_eq!((some, none), (6, 3));
    stop(sim, conn).await;
}
