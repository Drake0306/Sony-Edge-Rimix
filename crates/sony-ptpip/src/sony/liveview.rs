//! Live view: JPEG frames with focus frames and the level gauge, read with
//! GetObject on the live view handle (0xFFFFC002).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::ptp::codes::{
    HANDLE_LIVE_VIEW, OP_GET_OBJECT, OP_GET_OBJECT_INFO, RC_ACCESS_DENIED, RC_DEVICE_BUSY,
    RC_INVALID_OBJECT_HANDLE,
};
use crate::PtpIpConnection;

/// One autofocus frame, normalised to 0..1 of the image.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FocusFrame {
    /// 1 for a normal frame, 2 for the focused (main) frame.
    pub kind: u16,
    pub state: u16,
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
}

/// One live view frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveViewFrame {
    pub jpeg: Vec<u8>,
    pub focus_frames: Vec<FocusFrame>,
    /// Roll from the camera's level gauge, in degrees.
    pub level_deg: Option<f32>,
}

/// Oldest metadata version with the layout read here.
const MIN_META_VERSION: u16 = 0x64;
/// Metadata offset of the level gauge block.
const LEVEL_BLOCK: usize = 0x10;
/// Metadata offset of the first frame block (the focus frames).
const FRAME_BLOCK: usize = 0x28;
/// Bytes from a frame block's start to its first entry.
const FRAME_BLOCK_HEADER: usize = 16;
const FRAME_ENTRY_LEN: usize = 24;
/// Most focus frames read from one block.
const MAX_FRAMES: usize = 64;
/// Level gauge state meaning the reading is valid.
const LEVEL_VALID: u32 = 2;
/// How often an unavailable live view is logged.
const INVALID_HANDLE_LOG_EVERY: Duration = Duration::from_secs(5);

fn u16_at(buf: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        buf.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(buf: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        buf.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn i32_at(buf: &[u8], at: usize) -> Option<i32> {
    u32_at(buf, at).map(|v| v as i32)
}

/// The `[offset, offset + size)` range of `buf`, or a protocol error.
fn section<'a>(buf: &'a [u8], offset: u32, size: u32, what: &str) -> Result<&'a [u8], Error> {
    let start = offset as usize;
    start
        .checked_add(size as usize)
        .and_then(|end| buf.get(start..end))
        .ok_or_else(|| {
            Error::Protocol(format!(
                "live view {what} at {offset}+{size} is outside the {}-byte dataset",
                buf.len()
            ))
        })
}

/// Parses a live view dataset: a 16-byte header, then the JPEG and the
/// metadata at the offsets it names. Returns `None` when the camera has no
/// image yet.
///
/// The metadata is read on a best-effort basis: if it cannot be parsed the
/// frame still carries its JPEG, with no focus frames and no level.
pub fn parse_live_view_dataset(buf: &[u8]) -> Result<Option<LiveViewFrame>, Error> {
    // Header: image offset, image size, metadata offset, metadata size.
    let header = |i: usize| {
        u32_at(buf, i * 4).ok_or_else(|| {
            Error::Protocol(format!(
                "live view dataset of {} bytes has no header",
                buf.len()
            ))
        })
    };
    let (image_offset, image_size) = (header(0)?, header(1)?);
    let (meta_offset, meta_size) = (header(2)?, header(3)?);
    if image_size == 0 {
        return Ok(None);
    }
    let jpeg = section(buf, image_offset, image_size, "image")?;
    if !jpeg.starts_with(&[0xFF, 0xD8]) {
        return Err(Error::Protocol("live view image is not a JPEG".into()));
    }
    let meta = section(buf, meta_offset, meta_size, "metadata")?;
    let (focus_frames, level_deg) = parse_metadata(meta).unwrap_or_default();
    Ok(Some(LiveViewFrame {
        jpeg: jpeg.to_vec(),
        focus_frames,
        level_deg,
    }))
}

/// Reads the focus frames and the level; `None` if the metadata is malformed.
fn parse_metadata(meta: &[u8]) -> Option<(Vec<FocusFrame>, Option<f32>)> {
    if meta.is_empty() {
        return Some((Vec::new(), None));
    }
    // Older layouts place the blocks elsewhere; read none of them.
    if u16_at(meta, 0)? < MIN_META_VERSION {
        return Some((Vec::new(), None));
    }

    let state = u32_at(meta, LEVEL_BLOCK)?;
    let x = i32_at(meta, LEVEL_BLOCK + 8)?;
    // Assumption: the roll is reported in thousandths of a degree.
    // i32::MAX marks an axis the camera does not measure.
    let level_deg = (state == LEVEL_VALID && x != i32::MAX).then(|| x as f32 / 1000.0);

    let x_max = u32_at(meta, FRAME_BLOCK)?;
    let y_max = u32_at(meta, FRAME_BLOCK + 4)?;
    let count = usize::from(u16_at(meta, FRAME_BLOCK + 8)?).min(MAX_FRAMES);
    if x_max == 0 || y_max == 0 {
        return None;
    }
    let (x_max, y_max) = (x_max as f32, y_max as f32);
    let focus_frames = (0..count)
        .map(|i| {
            let e = FRAME_BLOCK + FRAME_BLOCK_HEADER + i * FRAME_ENTRY_LEN;
            Some(FocusFrame {
                kind: u16_at(meta, e)?,
                state: u16_at(meta, e + 2)?,
                cx: u32_at(meta, e + 8)? as f32 / x_max,
                cy: u32_at(meta, e + 12)? as f32 / y_max,
                h: u32_at(meta, e + 16)? as f32 / y_max,
                w: u32_at(meta, e + 20)? as f32 / x_max,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some((focus_frames, level_deg))
}

/// Checks that the camera offers live view. Call once per connection after
/// the SDIO handshake.
pub async fn start(conn: &mut PtpIpConnection) -> Result<(), Error> {
    match conn
        .command_data_in(OP_GET_OBJECT_INFO, &[HANDLE_LIVE_VIEW])
        .await
    {
        Ok(_) => Ok(()),
        Err(Error::Response {
            code: RC_INVALID_OBJECT_HANDLE,
            ..
        }) => Err(Error::Unsupported("live view not available")),
        Err(e) => Err(e),
    }
}

/// Reads the next live view frame. `None` means no frame is ready yet; wait
/// about 15 ms and ask again.
pub async fn next_frame(conn: &mut PtpIpConnection) -> Result<Option<LiveViewFrame>, Error> {
    match conn
        .command_data_in(OP_GET_OBJECT, &[HANDLE_LIVE_VIEW])
        .await
    {
        Ok((data, _)) => parse_live_view_dataset(&data),
        Err(Error::Response {
            code: RC_ACCESS_DENIED | RC_DEVICE_BUSY,
            ..
        }) => Ok(None),
        Err(Error::Response {
            code: RC_INVALID_OBJECT_HANDLE,
            ..
        }) => {
            log_invalid_handle();
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// Logs that live view is unavailable, at most once every five seconds.
fn log_invalid_handle() {
    static LAST_LOG: Mutex<Option<Instant>> = Mutex::new(None);
    let mut last = LAST_LOG.lock().unwrap_or_else(|e| e.into_inner());
    if last.is_none_or(|t| t.elapsed() >= INVALID_HANDLE_LOG_EVERY) {
        *last = Some(Instant::now());
        tracing::debug!("live view handle not available yet");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JPEG: [u8; 6] = [0xFF, 0xD8, 0xFF, 0xE0, 0xFF, 0xD9];

    /// Metadata as the simulator builds it: level 1.2°, one focused frame in
    /// the middle.
    fn metadata() -> Vec<u8> {
        let mut m = vec![0u8; 0x28];
        m[0..2].copy_from_slice(&0x0068u16.to_le_bytes());
        m[0x10..0x14].copy_from_slice(&2u32.to_le_bytes()); // state
        m[0x18..0x1C].copy_from_slice(&1200i32.to_le_bytes()); // x
        m[0x20..0x24].copy_from_slice(&i32::MAX.to_le_bytes()); // z
        m.extend(655_360u32.to_le_bytes()); // x_max
        m.extend(491_520u32.to_le_bytes()); // y_max
        m.extend(1u16.to_le_bytes()); // count
        m.extend([0u8; 6]); // pad to 16
        m.extend(2u16.to_le_bytes()); // type
        m.extend(2u16.to_le_bytes()); // state
        m.extend(0u32.to_le_bytes()); // reserved
        m.extend(327_680u32.to_le_bytes()); // cx
        m.extend(245_760u32.to_le_bytes()); // cy
        m.extend(122_880u32.to_le_bytes()); // h
        m.extend(163_840u32.to_le_bytes()); // w
        m
    }

    fn dataset(meta: &[u8], jpeg: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend((16 + meta.len() as u32).to_le_bytes());
        b.extend((jpeg.len() as u32).to_le_bytes());
        b.extend(16u32.to_le_bytes());
        b.extend((meta.len() as u32).to_le_bytes());
        b.extend(meta);
        b.extend(jpeg);
        b
    }

    #[test]
    fn parses_dataset_with_one_focus_frame() {
        let frame = parse_live_view_dataset(&dataset(&metadata(), &JPEG))
            .unwrap()
            .unwrap();
        assert_eq!(frame.jpeg, JPEG);
        assert_eq!(frame.level_deg, Some(1.2));
        assert_eq!(
            frame.focus_frames,
            vec![FocusFrame {
                kind: 2,
                state: 2,
                cx: 0.5,
                cy: 0.5,
                w: 0.25,
                h: 0.25,
            }]
        );
    }

    #[test]
    fn zero_image_size_is_none() {
        assert_eq!(
            parse_live_view_dataset(&dataset(&metadata(), &[])).unwrap(),
            None
        );
    }

    #[test]
    fn bad_jpeg_marker_is_protocol_error() {
        let err = parse_live_view_dataset(&dataset(&metadata(), &[0x00, 0xD8, 0xFF])).unwrap_err();
        assert!(matches!(err, Error::Protocol(_)), "{err:?}");
    }

    #[test]
    fn out_of_range_offsets_are_protocol_errors() {
        let mut b = dataset(&metadata(), &JPEG);
        b[4..8].copy_from_slice(&1000u32.to_le_bytes()); // image_size past the end
        assert!(matches!(
            parse_live_view_dataset(&b),
            Err(Error::Protocol(_))
        ));
        assert!(matches!(
            parse_live_view_dataset(&[0u8; 8]),
            Err(Error::Protocol(_))
        ));
    }

    #[test]
    fn metadata_parse_failure_still_returns_jpeg() {
        // Frame count claims more entries than the metadata holds.
        let mut meta = metadata();
        meta[0x30..0x32].copy_from_slice(&5u16.to_le_bytes());
        let frame = parse_live_view_dataset(&dataset(&meta, &JPEG))
            .unwrap()
            .unwrap();
        assert_eq!(frame.jpeg, JPEG);
        assert!(frame.focus_frames.is_empty());
        assert_eq!(frame.level_deg, None);

        // Metadata too short to hold even the level block.
        let frame = parse_live_view_dataset(&dataset(&[0x68, 0x00, 0x01], &JPEG))
            .unwrap()
            .unwrap();
        assert_eq!(frame.jpeg, JPEG);
        assert!(frame.focus_frames.is_empty());
        assert_eq!(frame.level_deg, None);
    }

    #[test]
    fn old_metadata_versions_are_ignored() {
        let mut meta = metadata();
        meta[0..2].copy_from_slice(&0x0063u16.to_le_bytes());
        let frame = parse_live_view_dataset(&dataset(&meta, &JPEG))
            .unwrap()
            .unwrap();
        assert!(frame.focus_frames.is_empty());
        assert_eq!(frame.level_deg, None);
    }

    #[test]
    fn level_is_none_unless_state_is_two() {
        let mut meta = metadata();
        meta[0x10..0x14].copy_from_slice(&1u32.to_le_bytes());
        let frame = parse_live_view_dataset(&dataset(&meta, &JPEG))
            .unwrap()
            .unwrap();
        assert_eq!(frame.level_deg, None);
        assert_eq!(frame.focus_frames.len(), 1);
    }
}
