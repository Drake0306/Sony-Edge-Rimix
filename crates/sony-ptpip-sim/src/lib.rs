//! A simulated Sony camera speaking PTP/IP, for tests and offline development.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sony_ptpip::ptp::PtpValue;
use tokio::sync::mpsc;

mod server;

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
    /// Property fixture file (used from Task 5).
    pub fixture: PathBuf,
    /// Hold back the response to this operation for this long (for timeout tests).
    pub delay_op: Option<(u16, Duration)>,
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig {
            model: "ZV-E10".into(),
            require_pairing: false,
            pairing_delay: Duration::ZERO,
            fixture: PathBuf::from("fixtures/zv-e10.json"),
            delay_op: None,
        }
    }
}

/// Instructions a test sends to the running simulator.
#[derive(Debug, Clone)]
pub enum SimCommand {
    /// Close both sockets of the current session.
    DropConnection,
    /// Change a property; emits a DevicePropChanged event for `code`.
    SetProperty { code: u16, value: PtpValue },
    /// Stop the listener and every session.
    Shutdown,
}

/// A running simulator.
pub struct SimHandle {
    pub addr: SocketAddr,
    pub events: mpsc::Sender<SimCommand>,
    /// One line per handled operation, e.g. `op 0x1002 tid 1`.
    pub log: Arc<Mutex<Vec<String>>>,
}
