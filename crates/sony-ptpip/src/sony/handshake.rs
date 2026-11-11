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
    let bytes = buf.get(*pos..*pos + 2).ok_or_else(truncated)?;
    *pos += 2;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn truncated() -> Error {
    Error::Protocol("ExtDeviceInfo truncated".into())
}

/// Reads a PTP array of `u16` codes: a `u32` count, then the codes. If that
/// count would run past the end of `buf`, reads the count as a `u16`
/// instead (the layout of older hand-made fixtures).
fn u16_list(buf: &[u8], pos: &mut usize, what: &str) -> Result<Vec<u16>, Error> {
    // True if `n` codes fit after a count of `count_len` bytes at `at`.
    let fits = |at: usize, count_len: usize, n: usize| {
        n.checked_mul(2)
            .and_then(|b| b.checked_add(at + count_len))
            .is_some_and(|end| end <= buf.len())
    };
    let wide = buf
        .get(*pos..*pos + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let n = match wide {
        Some(n) if fits(*pos, 4, n) => {
            *pos += 4;
            n
        }
        _ => {
            let n = u16_at(buf, pos)? as usize;
            if !fits(*pos, 0, n) {
                return Err(truncated());
            }
            tracing::debug!(what, n, "ExtDeviceInfo array has a u16 count; read as such");
            n
        }
    };
    (0..n).map(|_| u16_at(buf, pos)).collect()
}

/// Parses the 0x9202 data phase: a `u16` version, then the property codes
/// and, if any bytes remain, the control codes, each as a PTP array with a
/// `u32` count.
pub fn parse_ext_device_info(buf: &[u8]) -> Result<ExtDeviceInfo, Error> {
    let mut pos = 0;
    let protocol_version = u16_at(buf, &mut pos)?;
    let properties = u16_list(buf, &mut pos, "properties")?;
    let controls = if pos < buf.len() {
        u16_list(buf, &mut pos, "controls")?
    } else {
        Vec::new()
    };
    Ok(ExtDeviceInfo {
        protocol_version,
        properties,
        controls,
    })
}

/// Runs the three-phase Sony SDIO handshake and returns the camera's
/// extension info. Sony operations are refused until this completes.
pub async fn sdio_connect(conn: &mut PtpIpConnection) -> Result<ExtDeviceInfo, Error> {
    connect_phase(conn, 1).await?;
    connect_phase(conn, 2).await?;
    let (data, _) = conn
        .command_data_in(OP_SDIO_GET_EXT_DEVICE_INFO, &[PROTOCOL_VERSION])
        .await?;
    let info = parse_ext_device_info(&data)?;
    connect_phase(conn, 3).await?;
    Ok(info)
}

/// Sends one SDIO_Connect (0x9201) phase. Cameras such as the ZV-E10 answer
/// it with a data phase before the response, others with none; both are
/// accepted.
async fn connect_phase(conn: &mut PtpIpConnection, phase: u32) -> Result<(), Error> {
    let (data, _) = conn
        .command_data_in(OP_SDIO_CONNECT, &[phase, 0, 0])
        .await?;
    if !data.is_empty() {
        let hex: String = data.iter().map(|b| format!("{b:02x}")).collect();
        tracing::debug!(phase, data = %hex, "SDIO_Connect answered with data");
    }
    Ok(())
}
