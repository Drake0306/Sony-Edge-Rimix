//! A simulated Sony camera speaking PTP/IP, for tests and offline development.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sony_ptpip::ptp::PtpValue;
use tokio::sync::mpsc;

mod controls;
mod frames;
mod props;
mod server;
mod ssdp;

pub use props::load_fixture;
pub use server::spawn;

/// How the simulated camera behaves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimConfig {
    /// Model name reported in InitCommandAck and DeviceInfo.
    pub model: String,
    /// Whether the camera holds back InitCommandAck as if showing a pairing prompt.
    pub require_pairing: bool,
    /// How long the pairing prompt takes to be accepted.
    pub pairing_delay: Duration,
    /// Property fixture file; relative paths are resolved against this crate.
    pub fixture: PathBuf,
    /// Hold back the response to this operation for this long (for timeout tests).
    pub delay_op: Option<(u16, Duration)>,
    /// Refuse every 0x9205 write with InvalidParameter, as cameras that only
    /// take stepped writes do.
    pub write_mode_step: bool,
    /// Answer every Nth GetObject on the live view handle with AccessDenied,
    /// as a camera does while it has no new frame.
    pub live_view_busy_every: Option<u32>,
    /// Cut GetObject on the captured still handle short: send StartData and
    /// the first half of the picture, then close both sockets.
    pub drop_during_capture: bool,
    /// When a session ends with [`SimCommand::DropConnection`], put the most
    /// recently fetched picture back in the capture queue, as a camera does
    /// when the link drops before it learns the transfer finished.
    pub redeliver_after_drop: bool,
    /// After this many pictures have been fetched, numbering starts over at
    /// DSC00001, as after formatting the card. Later pictures differ byte for
    /// byte from earlier ones with the same number but keep their size.
    pub restart_numbering_after: Option<u32>,
    /// Answer the first this-many GetObjectInfo requests on the live view
    /// handle with AccessDenied, as a camera does while live view starts up.
    pub live_view_info_denied: u32,
    /// Refuse the Nth 0x9207 control of the simulator's life (counting from
    /// 1) with DeviceBusy, without applying it.
    pub fail_nth_control: Option<u32>,
    /// Send each captured still in five Data packets with this pause before
    /// each one after the first, as a slow link does.
    pub capture_packet_pause: Option<Duration>,
    /// How many files one shutter release produces, as RAW+JPEG gives two.
    pub files_per_shot: u32,
    /// Answer SSDP M-SEARCH requests on this 127.0.0.1 UDP port (0 picks
    /// one) and serve the UPnP descriptions over HTTP; `None` disables both.
    pub ssdp_port: Option<u16>,
    /// Where to listen for PTP/IP; `None` picks a free port on 127.0.0.1.
    pub bind_addr: Option<SocketAddr>,
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig {
            model: "ZV-E10".into(),
            require_pairing: false,
            pairing_delay: Duration::ZERO,
            fixture: PathBuf::from("fixtures/zv-e10.json"),
            delay_op: None,
            write_mode_step: false,
            live_view_busy_every: None,
            drop_during_capture: false,
            redeliver_after_drop: false,
            restart_numbering_after: None,
            live_view_info_denied: 0,
            fail_nth_control: None,
            capture_packet_pause: None,
            files_per_shot: 1,
            ssdp_port: None,
            bind_addr: None,
        }
    }
}

/// Instructions a test sends to the running simulator.
#[derive(Debug, Clone)]
pub enum SimCommand {
    /// Close both sockets of the current session.
    DropConnection,
    /// Change a property as if on the camera body; emits a DevicePropChanged
    /// event for `code` when its value changes.
    SetProperty { code: u16, value: PtpValue },
    /// Send a PTP/IP ProbeRequest on the event socket. A ProbeResponse that
    /// comes back is logged as `event ProbeResponse`.
    Probe,
    /// Stop the listener and every session.
    Shutdown,
}

/// A running simulator.
pub struct SimHandle {
    pub addr: SocketAddr,
    pub events: mpsc::Sender<SimCommand>,
    /// One line per handled operation, e.g. `op 0x1002 tid 1`. Controls
    /// (0x9207) add the control and value, e.g. `op 0x9207 tid 9 data-out 2
    /// D2C1=2`; a refused one ends with ` refused`. Packets read from the
    /// event socket are logged as `event <packet>`.
    pub log: Arc<Mutex<Vec<String>>>,
    /// Where the UPnP descriptions are served, when `ssdp_port` is set.
    pub http_addr: Option<SocketAddr>,
    /// Where the SSDP responder listens, when `ssdp_port` is set.
    pub ssdp_addr: Option<SocketAddr>,
}
