//! Sony vendor extensions on top of PTP.

pub mod handshake;

pub use handshake::{sdio_connect, ExtDeviceInfo};
