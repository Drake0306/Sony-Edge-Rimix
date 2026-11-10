pub mod camera;
pub mod discovery;
pub mod error;
pub mod guid;
pub mod ptp;
pub mod ptpip;
pub mod quirks;
pub mod sony;
pub mod ssh;

pub use camera::{
    BoxFuture, Camera, CameraConfig, CameraEvent, CameraState, Choice, PropertySummary,
    ReconnectHook,
};
pub use discovery::{discover, DiscoveredCamera, DiscoverySource};
pub use guid::{default_host_name, load_or_create_guid};
pub use ptpip::transport::{ConnectOptions, PtpEvent, PtpIpConnection};
pub use quirks::{quirks_for, quirks_from, CardBrowse, Generation, Quirks, WriteMode};
pub use sony::{focus, sdio_connect, set_value, shoot, toggle_record, ExtDeviceInfo};
pub use sony::{CardKind, CardObject};
pub use ssh::{normalize_fingerprint, SshConfig, SshTunnel};

/// Live view frames: `liveview::start` once after the handshake, then
/// `liveview::next_frame` in a loop.
pub mod liveview {
    pub use crate::sony::liveview::{
        next_frame, parse_live_view_dataset, start, FocusFrame, LiveViewFrame,
    };
}

/// Receiving new shots: `capture::captured_ready` to see how many wait, then
/// `capture::receive_one` for each.
pub mod capture {
    pub use crate::sony::capture::{
        captured_ready, default_recovery_dir, receive_one, Destination, DestinationMode, Received,
    };
}

/// Browsing the memory card: `card::list_storages`, `card::list_objects`,
/// then `card::download` for each file wanted.
pub mod card {
    pub use crate::sony::card::{
        download, list_objects, list_storages, object, object_handles, parse_ptp_date, CardKind,
        CardObject, PartialDownload, CARD_CHUNK,
    };
}
