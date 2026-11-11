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
    /// The faces (or animal faces) the camera has found.
    #[serde(default)]
    pub face_frames: Vec<FocusFrame>,
    /// The subject the camera is tracking.
    #[serde(default)]
    pub tracking_frames: Vec<FocusFrame>,
    /// Roll from the camera's level gauge, in degrees.
    pub level_deg: Option<f32>,
    /// The metadata as the camera sent it, for the log.
    #[serde(skip)]
    pub raw_meta: Vec<u8>,
}

/// Oldest metadata version with the layout read here.
const MIN_META_VERSION: u16 = 0x64;
/// Metadata offset of the level gauge block.
const LEVEL_BLOCK: usize = 0x10;
/// Metadata offset of the first frame block (the focus frames), as the
/// ZV-E10 sends it (metadata version 0x65).
const FRAME_BLOCK: usize = 0x38;
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
    let raw_meta = section(buf, meta_offset, meta_size, "metadata").unwrap_or_default();
    let (blocks, level_deg) = parse_metadata(raw_meta).unwrap_or_default();
    let [focus_frames, face_frames, tracking_frames] = blocks;
    Ok(Some(LiveViewFrame {
        jpeg: jpeg.to_vec(),
        focus_frames,
        face_frames,
        tracking_frames,
        level_deg,
        raw_meta: raw_meta.to_vec(),
    }))
}

/// The focus frames, the face frames and the tracking frames, in that order.
type FrameBlocks = [Vec<FocusFrame>; 3];

/// Reads the frames and the level; `None` if the metadata is malformed.
///
/// After the level come blocks of frames, one after another: focus frames,
/// then faces, then the tracked subject (the ZV-E10 sends a fourth, which
/// is not read). Each block is a 16-byte header (the size of the grid its
/// frames are measured on, 640 by 480 times 1024 on the ZV-E10, then how
/// many frames) and 24 bytes per frame. The offsets and the first block are
/// as a real ZV-E10 sent them; which of the later blocks holds faces and
/// which the tracked subject follows Sony's SDK and is not yet seen filled.
fn parse_metadata(meta: &[u8]) -> Option<(FrameBlocks, Option<f32>)> {
    if meta.is_empty() {
        return Some((FrameBlocks::default(), None));
    }
    // Older layouts place the blocks elsewhere; read none of them.
    if u16_at(meta, 0)? < MIN_META_VERSION {
        return Some((FrameBlocks::default(), None));
    }

    let state = u32_at(meta, LEVEL_BLOCK)?;
    let x = i32_at(meta, LEVEL_BLOCK + 8)?;
    // The roll comes in whole degrees on a scale that wraps at 360 (the
    // ZV-E10 sends 359, then 0, when it is nearly level), so the upper half
    // is a tilt the other way. i32::MAX marks an axis the camera does not
    // measure.
    let level_deg =
        (state == LEVEL_VALID && x != i32::MAX).then(|| if x > 180 { x - 360 } else { x } as f32);

    let mut blocks = FrameBlocks::default();
    let mut at = FRAME_BLOCK;
    let (first, next) = frame_block(meta, at)?;
    blocks[0] = first;
    at = next;
    for block in blocks.iter_mut().skip(1) {
        // A block that is cut short, or whose frames fall outside the
        // picture, is not one: stop there and keep what was read.
        match frame_block(meta, at) {
            Some((frames, next)) if frames.iter().all(plausible) => {
                *block = frames;
                at = next;
            }
            _ => break,
        }
    }
    Some((blocks, level_deg))
}

/// One block of frames at `at`, and where the next block would start.
fn frame_block(meta: &[u8], at: usize) -> Option<(Vec<FocusFrame>, usize)> {
    let x_max = u32_at(meta, at)?;
    let y_max = u32_at(meta, at + 4)?;
    let count = usize::from(u16_at(meta, at + 8)?);
    if x_max == 0 || y_max == 0 || count > MAX_FRAMES {
        return None;
    }
    let (x_max, y_max) = (x_max as f32, y_max as f32);
    let frames = (0..count)
        .map(|i| {
            let e = at + FRAME_BLOCK_HEADER + i * FRAME_ENTRY_LEN;
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
    Some((frames, at + FRAME_BLOCK_HEADER + count * FRAME_ENTRY_LEN))
}

/// Whether a frame lies inside the picture and has a size.
fn plausible(f: &FocusFrame) -> bool {
    let inside = |v: f32| (0.0..=1.0).contains(&v);
    inside(f.cx) && inside(f.cy) && f.w > 0.0 && f.w <= 1.0 && f.h > 0.0 && f.h <= 1.0
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

    /// What the ZV-E10 (firmware 2.01) sent on 2026-11-11, from the app's
    /// log: nothing in any of its four frame blocks, tilted 1° to the left.
    const REAL_NO_FRAMES: &str = "65000100000000000100000000000000020000000000000067010000fbfffffffbffffff000000000000000000000000000000000000000000000a0000800700000000000000000000000a0000800700000000000000000000000a0000800700000000000000000000000a00008007000000000000000000";
    /// The same body a moment later, with one focus frame up and to the right.
    const REAL_ONE_FRAME: &str = "650001000000000001000000000000000200000000000000660100000000000000000000000000000000000000000000000000000000000000000a00008007000100000000000000060001000200000000c0080007d701003ef200000010010000000a0000800700000000000000000000000a0000800700000000000000000000000a00008007000000000000000000";

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn reads_what_a_real_camera_sent() {
        let frame = parse_live_view_dataset(&dataset(&unhex(REAL_ONE_FRAME), &JPEG))
            .unwrap()
            .unwrap();
        assert_eq!(frame.focus_frames.len(), 1, "{frame:?}");
        let f = &frame.focus_frames[0];
        assert_eq!((f.kind, f.state), (6, 1));
        // 573440 of 655360 across, 120583 of 491520 down.
        assert!((f.cx - 0.875).abs() < 0.0005, "{f:?}");
        assert!((f.cy - 0.2453).abs() < 0.0005, "{f:?}");
        assert!((f.w - 0.10625).abs() < 0.0005, "{f:?}");
        assert!((f.h - 0.1262).abs() < 0.0005, "{f:?}");
        assert!(frame.face_frames.is_empty() && frame.tracking_frames.is_empty());
        // 358 on a scale that wraps at 360: two degrees to the left.
        assert_eq!(frame.level_deg, Some(-2.0));

        let frame = parse_live_view_dataset(&dataset(&unhex(REAL_NO_FRAMES), &JPEG))
            .unwrap()
            .unwrap();
        assert!(frame.focus_frames.is_empty());
        assert!(frame.face_frames.is_empty() && frame.tracking_frames.is_empty());
        assert_eq!(frame.level_deg, Some(-1.0));
    }

    #[test]
    fn level_is_in_whole_degrees_either_way() {
        for (sent, shown) in [
            (0, 0.0),
            (3, 3.0),
            (180, 180.0),
            (181, -179.0),
            (359, -1.0),
            (-4, -4.0),
        ] {
            let mut meta = metadata();
            meta[0x18..0x1C].copy_from_slice(&i32::to_le_bytes(sent));
            let frame = parse_live_view_dataset(&dataset(&meta, &JPEG))
                .unwrap()
                .unwrap();
            assert_eq!(frame.level_deg, Some(shown), "{sent}");
        }
    }

    /// Metadata as the simulator builds it: tilted 2° to the left, one
    /// focused frame in the middle.
    fn metadata() -> Vec<u8> {
        let mut m = vec![0u8; 0x38];
        m[0..2].copy_from_slice(&0x0065u16.to_le_bytes());
        m[0x10..0x14].copy_from_slice(&2u32.to_le_bytes()); // state
        m[0x18..0x1C].copy_from_slice(&358i32.to_le_bytes()); // x
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
        assert_eq!(frame.level_deg, Some(-2.0));
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
        meta[0x40..0x42].copy_from_slice(&5u16.to_le_bytes());
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
    fn metadata_range_out_of_bounds_still_returns_jpeg() {
        for (offset, size) in [(16, 1000), (5000, 0)] {
            let mut b = dataset(&metadata(), &JPEG);
            b[8..12].copy_from_slice(&(offset as u32).to_le_bytes());
            b[12..16].copy_from_slice(&(size as u32).to_le_bytes());
            let frame = parse_live_view_dataset(&b).unwrap().unwrap();
            assert_eq!(&frame.jpeg[..2], &[0xFF, 0xD8]);
            assert!(frame.focus_frames.is_empty());
            assert_eq!(frame.level_deg, None);
        }
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
