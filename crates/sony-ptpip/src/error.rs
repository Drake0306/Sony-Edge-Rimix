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
    #[error("camera disconnected")]
    Disconnected,
    #[error("unsupported: {0}")]
    Unsupported(&'static str),
}
