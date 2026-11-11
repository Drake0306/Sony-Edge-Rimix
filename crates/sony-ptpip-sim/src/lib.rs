//! A simulated Sony camera speaking PTP/IP, for tests and offline development.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sony_ptpip::ptp::PtpValue;
use tokio::sync::mpsc;

mod card;
mod controls;
mod frames;
mod props;
mod server;
mod ssdp;
mod ssh;

pub use card::SimCardFile;
pub use props::load_fixture;
pub use server::spawn;
pub use ssh::SshSimConfig;

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
    /// Put an SSH server in front of the PTP/IP port, as cameras with
    /// Access Authentication do; `None` leaves PTP/IP open directly.
    pub ssh: Option<SshSimConfig>,
    /// Refuse `direct-tcpip` forwards to this host string with
    /// AdministrativelyProhibited, as cameras that only accept one spelling
    /// of their own address do. `None` accepts any host.
    #[serde(default)]
    pub ssh_reject_host: Option<String>,
    /// The files on the memory card, on one storage. Their folders are
    /// served as associations, and GetObjectHandles answers per parent (the
    /// root holds only top-level folders and root files). Their bytes are
    /// generated: byte `i` of the object with handle `h` is
    /// `(i * 31 + h) as u8`.
    #[serde(default = "card::default_card")]
    pub card: Vec<SimCardFile>,
    /// Answer GetObjectInfo on the card file with this name with
    /// AccessDenied, as a camera does for a file it cannot describe.
    #[serde(default)]
    pub card_info_fails_for: Option<String>,
    /// Once in the simulator's life, close both sockets after this many
    /// bytes of card data (GetPartialObject or GetObject) have been sent.
    #[serde(default)]
    pub drop_during_card_download_at: Option<u64>,
    /// Answer each accepted SDIO_Connect phase (0x9201) with 8 bytes of
    /// data before the response, as the ZV-E10 does; `false` sends a bare
    /// response.
    #[serde(default = "default_true")]
    pub sdio_connect_data: bool,
    /// Never answer 0x9209 (SDIO_GetAllExtDevicePropInfo) sent with any
    /// parameter: the request is read and then nothing more comes until the
    /// connection drops. `false` answers parameter 0 with every property and
    /// 1 with the properties changed since the last such read, as the
    /// simulator did before. 0x9209 with no parameters (the form libgphoto2
    /// uses) is always answered with every property.
    #[serde(default = "default_true")]
    pub props_param_hangs: bool,
    /// Never answer 0x9251 (SDIO_GetExtDeviceProp), as a body that does not
    /// implement it may do. `false` answers it with the one property.
    #[serde(default = "default_true")]
    pub single_prop_read_hangs: bool,
}

fn default_true() -> bool {
    true
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
            ssh: None,
            ssh_reject_host: None,
            card: card::default_card(),
            card_info_fails_for: None,
            drop_during_card_download_at: None,
            sdio_connect_data: true,
            props_param_hangs: true,
            single_prop_read_hangs: true,
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
    /// Change the password the SSH front accepts, as resetting the camera's
    /// Access Authentication does. Ignored without an SSH front.
    SetSshPassword(String),
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
    /// Where the SSH front listens, when `ssh` is set.
    pub ssh_addr: Option<SocketAddr>,
    /// The SSH host key's fingerprint in OpenSSH form, `SHA256:<base64>`.
    pub fingerprint_sha256: Option<String>,
    /// The SSH host key's MD5 fingerprint, `aa:bb:...`.
    pub fingerprint_md5: Option<String>,
}
