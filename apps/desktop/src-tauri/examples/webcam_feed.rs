//! Serves the simulator's live view as the webcam feed, without the app:
//!
//!   cargo run -p rimix-desktop --example webcam_feed
//!
//! then open the address it prints in a browser or add it to OBS. For
//! checking the feed against real viewers with no camera at hand.

use std::sync::Arc;

use rimix_desktop_lib::webcam::{FrameSource, WebcamService};
use sony_ptpip::capture::{Destination, DestinationMode};
use sony_ptpip::{Camera, CameraConfig};
use sony_ptpip_sim::{spawn, SimConfig};

#[tokio::main]
async fn main() {
    let sim = spawn(SimConfig::default()).await;
    let camera = Arc::new(
        Camera::connect(CameraConfig {
            addr: sim.addr,
            guid: [0x42; 16],
            host_name: "Rimix feed example".into(),
            destination: Destination {
                base: std::env::temp_dir(),
                mode: DestinationMode::CustomFolder,
                recovery_dir: None,
            },
            auto_receive: false,
            reconnect: true,
            before_reconnect: None,
            ssh: None,
        })
        .await
        .expect("connect to the simulator"),
    );
    let service = WebcamService::default();
    let source: FrameSource = {
        let camera = camera.clone();
        Arc::new(move || camera.latest_frame())
    };
    let addr = service.start(source).await.expect("start the feed");
    println!("feed: http://{addr}/");
    tokio::signal::ctrl_c().await.ok();
    service.stop();
    camera.disconnect().await;
}
