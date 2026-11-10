/// Errors produced by the PTP/IP layers.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("camera responded with code {code:#06x} to operation {op:#06x}")]
    Response { code: u16, op: u16 },
    #[error("operation timed out")]
    Timeout,
    /// The camera did not answer InitCommandRequest within the pairing wait,
    /// usually because nobody pressed OK on its pairing prompt.
    #[error("the camera did not confirm the pairing in time")]
    PairingTimeout,
    #[error("camera disconnected")]
    Disconnected,
    #[error("unsupported: {0}")]
    Unsupported(&'static str),
    /// A shot was downloaded but could not be written to its destination;
    /// it was saved at `path` (in the recovery folder) instead.
    #[error("{source}; the shot was kept in {}", .path.display())]
    Recovered {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
}
