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
    /// The camera's SSH server (Access Authentication) refused the login.
    #[error("the camera refused the username or password")]
    SshAuth,
    /// The camera's SSH host key matches neither fingerprint the user
    /// confirmed earlier.
    #[error("the camera's fingerprint changed (expected {expected}, got {actual_sha256})")]
    HostKeyMismatch {
        expected: String,
        actual_sha256: String,
    },
    /// No fingerprint was confirmed yet; show these to the user to compare
    /// with the camera's Access Authen. Info screen.
    #[error("the camera's fingerprint needs confirming: {sha256}")]
    HostKeyUnknown { sha256: String, md5: String },
}
