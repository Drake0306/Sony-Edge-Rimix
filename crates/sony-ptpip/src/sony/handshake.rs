use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::ptp::codes::{OP_SDIO_CONNECT, OP_SDIO_GET_EXT_DEVICE_INFO};
use crate::PtpIpConnection;

/// Protocol version requested from the camera in 0x9202.
const PROTOCOL_VERSION: u32 = 0x012C;

/// What the camera reports about its Sony extension in 0x9202.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtDeviceInfo {
    pub protocol_version: u16,
    pub properties: Vec<u16>,
    pub controls: Vec<u16>,
}

fn u16_at(buf: &[u8], pos: &mut usize) -> Result<u16, Error> {
    let bytes = buf
        .get(*pos..*pos + 2)
        .ok_or_else(|| Error::Protocol("ExtDeviceInfo truncated".into()))?;
    *pos += 2;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u16_list(buf: &[u8], pos: &mut usize) -> Result<Vec<u16>, Error> {
    let n = u16_at(buf, pos)?;
    (0..n).map(|_| u16_at(buf, pos)).collect()
}

/// Parses the 0x9202 data phase: version, property codes, control codes.
pub fn parse_ext_device_info(buf: &[u8]) -> Result<ExtDeviceInfo, Error> {
    let mut pos = 0;
    let protocol_version = u16_at(buf, &mut pos)?;
    let properties = u16_list(buf, &mut pos)?;
    let controls = u16_list(buf, &mut pos)?;
    Ok(ExtDeviceInfo {
        protocol_version,
        properties,
        controls,
    })
}

/// Runs the three-phase Sony SDIO handshake and returns the camera's
/// extension info. Sony operations are refused until this completes.
pub async fn sdio_connect(conn: &mut PtpIpConnection) -> Result<ExtDeviceInfo, Error> {
    conn.command(OP_SDIO_CONNECT, &[1, 0, 0]).await?;
    conn.command(OP_SDIO_CONNECT, &[2, 0, 0]).await?;
    let (data, _) = conn
        .command_data_in(OP_SDIO_GET_EXT_DEVICE_INFO, &[PROTOCOL_VERSION])
        .await?;
    let info = parse_ext_device_info(&data)?;
    conn.command(OP_SDIO_CONNECT, &[3, 0, 0]).await?;
    Ok(info)
}
