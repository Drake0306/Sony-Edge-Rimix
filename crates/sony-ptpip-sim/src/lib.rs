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
pub mod sendto;
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
    /// Start with this still image save destination (0xD222) instead of the
    /// fixture's: 0x0001 computer only, 0x0010 card only, 0x0011 both.
    /// While it leaves out the computer, a shutter release offers no shot
    /// (0xD215 stays 0), as a body set to "Camera Only" behaves.
    #[serde(default)]
    pub still_destination: Option<u16>,
    /// Answer the first this-many writes of the save destination (0xD222)
    /// with OK and change nothing, as the real ZV-E10 did right after
    /// connecting: Rimix wrote "computer and card", the camera said OK, and
    /// its next report still said "card only".
    #[serde(default)]
    pub still_destination_ignores_writes: u32,
    /// Properties reported without their list of values (form "none"), as
    /// the real ZV-E10 reports shutter speed (0xD20D) and aperture
    /// (0x5007). They still move one value at a time when stepped with
    /// 0x9207, through the fixture's list, and refuse a direct write.
    #[serde(default)]
    pub unlisted: Vec<u16>,
    /// Describe a waiting picture tersely: no file name, and the dataset
    /// ends right after the (empty) name, without the date strings. Some
    /// bodies do this (libgphoto2 names such files `capt0000.arw` itself).
    #[serde(default)]
    pub terse_capture_info: bool,
    /// Refuse every look at the card (GetStorageIDs answers AccessDenied),
    /// as a body that keeps its card to itself while it is remote
    /// controlled does.
    #[serde(default)]
    pub card_denied: bool,
    /// Start with this size of image sent to the computer (0xD268) instead
    /// of the fixture's: 1 original, 2 a small 2M copy.
    #[serde(default)]
    pub host_image_size: Option<u8>,
    /// Give the camera a focus area (0xD22C), which the fixtures leave out:
    /// its value (1 wide, 0x0102 Flexible Spot: M, as the ZV-E10 lists them)
    /// and whether it can be changed. In its automatic modes a real body
    /// fixes the area and refuses a change.
    #[serde(default)]
    pub focus_area: Option<(u16, bool)>,
    /// The focus areas the camera lists, when not all fourteen: once
    /// settled, the ZV-E10 listed only wide, zone and Flexible Spot: L
    /// (`[1, 2, 0x103]`).
    #[serde(default)]
    pub focus_areas: Vec<u16>,
    /// Give the camera Sony's zoom properties, which the fixtures leave
    /// out: whether the lens can be zoomed from the computer (0xD25B), with
    /// the zoom scale (0xD25C) and the zoom setting (0xD25F: optical only,
    /// Clear Image Zoom, digital zoom, as the ZV-E10 lists them).
    #[serde(default)]
    pub zoom: Option<bool>,
    /// List the remote touch control (0xD2E4) among the camera's controls,
    /// as the ZV-E10 does.
    #[serde(default)]
    pub remote_touch: bool,
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
            still_destination: None,
            still_destination_ignores_writes: 0,
            unlisted: Vec::new(),
            terse_capture_info: false,
            card_denied: false,
            host_image_size: None,
            focus_area: None,
            focus_areas: Vec::new(),
            zoom: None,
            remote_touch: false,
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
