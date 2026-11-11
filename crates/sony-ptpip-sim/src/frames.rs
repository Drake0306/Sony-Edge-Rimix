//! The simulator's pictures: live view (a moving test picture with focus
//! frame and level metadata, on handle 0xFFFFC002) and captured stills (on
//! handle 0xFFFFC001).

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, Rgb, RgbImage};
use sony_ptpip::ptp::codes::RC_ACCESS_DENIED;
use sony_ptpip::ptp::{encode_object_info, ObjectInfo};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 424;
const JPEG_QUALITY: u8 = 70;
const CIRCLE_RADIUS: i64 = 30;
/// How far the circle moves each frame, in pixels.
const CIRCLE_STEP: u32 = 8;
const BACKGROUND: Rgb<u8> = Rgb([128, 128, 128]);
const LIGHT: Rgb<u8> = Rgb([240, 240, 240]);
const DARK: Rgb<u8> = Rgb([32, 32, 32]);
/// The frame counter is drawn as this many binary stripes, lowest bit first.
const COUNTER_BITS: u32 = 16;
const FORMAT_JFIF: u16 = 0x3801;
const HEADER_LEN: u32 = 16;

/// Live view state, kept across sessions.
#[derive(Default)]
pub struct LiveView {
    /// GetObject calls on the live view handle so far.
    requests: u32,
    /// Frames served so far.
    frames: u32,
}

impl LiveView {
    /// Answers GetObject on the live view handle: the dataset to send, or the
    /// response code when there is none. With `busy_every` = N, every Nth call
    /// is refused with AccessDenied as if no new frame were ready.
    pub fn next(&mut self, busy_every: Option<u32>) -> Result<Vec<u8>, u16> {
        self.requests += 1;
        if busy_every.is_some_and(|n| n > 0 && self.requests.is_multiple_of(n)) {
            return Err(RC_ACCESS_DENIED);
        }
        let dataset = dataset(&render(self.frames));
        self.frames += 1;
        Ok(dataset)
    }
}

/// The ObjectInfo answered for the live view handle.
pub fn object_info() -> Vec<u8> {
    let info = ObjectInfo {
        storage_id: 0,
        object_format: FORMAT_JFIF,
        compressed_size: 0,
        filename: "LiveView.jpg".into(),
        capture_date: String::new(),
    };
    encode_object_info(&info)
}

/// Byte offset of the JFIF X density field in the encoder's output.
const JFIF_X_DENSITY: usize = 14;

/// The JPEG of captured shot `shot`, with the shot number drawn as stripes.
/// For `generation` above 0 (numbering has started over) the JFIF density
/// carries the generation, so the file differs from generation 0's shot of
/// the same number byte for byte while keeping its size.
pub fn capture_jpeg(shot: u32, generation: u32) -> Vec<u8> {
    let mut jpeg = render(shot);
    if generation > 0 {
        assert_eq!(
            &jpeg[6..11],
            b"JFIF\0",
            "encoder output starts with a JFIF header"
        );
        let density = u16::try_from(generation.saturating_add(1)).unwrap_or(u16::MAX);
        jpeg[JFIF_X_DENSITY..JFIF_X_DENSITY + 2].copy_from_slice(&density.to_be_bytes());
    }
    jpeg
}

/// The ObjectInfo answered for captured shot `shot` of `size` bytes.
pub fn capture_object_info(shot: u32, size: usize) -> Vec<u8> {
    let info = ObjectInfo {
        storage_id: 0,
        object_format: FORMAT_JFIF,
        compressed_size: u32::try_from(size).unwrap_or(u32::MAX),
        filename: format!("DSC{shot:05}.JPG"),
        capture_date: String::new(),
    };
    encode_object_info(&info)
}

/// Draws frame `n` and encodes it as a JPEG.
fn render(n: u32) -> Vec<u8> {
    let mut img = RgbImage::from_pixel(WIDTH, HEIGHT, BACKGROUND);
    let cx = i64::from(n.wrapping_mul(CIRCLE_STEP) % WIDTH);
    let cy = i64::from(HEIGHT / 2);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let (dx, dy) = (i64::from(x) - cx, i64::from(y) - cy);
        if dx * dx + dy * dy <= CIRCLE_RADIUS * CIRCLE_RADIUS {
            *px = LIGHT;
        }
    }
    for bit in 0..COUNTER_BITS {
        let colour = if n >> bit & 1 == 1 { LIGHT } else { DARK };
        let x0 = 8 + bit * 20;
        for x in x0..x0 + 16 {
            for y in 8..24 {
                img.put_pixel(x, y, colour);
            }
        }
    }
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY)
        .encode(img.as_raw(), WIDTH, HEIGHT, ExtendedColorType::Rgb8)
        .expect("encode live view JPEG");
    jpeg
}

/// Live view metadata, laid out as the ZV-E10 sends it: tilted 2° to the
/// left, one focused frame in the middle, a face up and to the left, a
/// tracked subject down and to the right, and a fourth, empty block.
fn metadata() -> Vec<u8> {
    let mut m = vec![0u8; 0x38];
    m[0..2].copy_from_slice(&0x0065u16.to_le_bytes()); // version
    m[2..4].copy_from_slice(&1u16.to_le_bytes());
    m[8..12].copy_from_slice(&1u32.to_le_bytes());
    m[0x10..0x14].copy_from_slice(&2u32.to_le_bytes()); // level state: valid
    m[0x18..0x1C].copy_from_slice(&358i32.to_le_bytes()); // x: 358 of 360, 2° left
    m[0x1C..0x20].copy_from_slice(&0i32.to_le_bytes()); // y
    m[0x20..0x24].copy_from_slice(&i32::MAX.to_le_bytes()); // z: not measured
    m.extend(655_360u32.to_le_bytes()); // x_max
    m.extend(491_520u32.to_le_bytes()); // y_max
    m.extend(1u16.to_le_bytes()); // count
    m.extend([0u8; 6]); // pad to 16 bytes from the block start
    m.extend(2u16.to_le_bytes()); // type: focused
    m.extend(2u16.to_le_bytes()); // state
    m.extend(0u32.to_le_bytes()); // reserved
    m.extend(327_680u32.to_le_bytes()); // cx
    m.extend(245_760u32.to_le_bytes()); // cy
    m.extend(122_880u32.to_le_bytes()); // h
    m.extend(163_840u32.to_le_bytes()); // w
                                        // Then the faces and the tracked subject, each a block of the same shape.
    for (cx, cy, h, w) in [
        (196_608u32, 147_456u32, 98_304u32, 98_304u32),
        (458_752, 294_912, 147_456, 131_072),
    ] {
        m.extend(655_360u32.to_le_bytes()); // x_max
        m.extend(491_520u32.to_le_bytes()); // y_max
        m.extend(1u16.to_le_bytes()); // count
        m.extend([0u8; 6]);
        m.extend(1u16.to_le_bytes()); // type
        m.extend(1u16.to_le_bytes()); // state
        m.extend(0u32.to_le_bytes());
        m.extend(cx.to_le_bytes());
        m.extend(cy.to_le_bytes());
        m.extend(h.to_le_bytes());
        m.extend(w.to_le_bytes());
    }
    m.extend(655_360u32.to_le_bytes());
    m.extend(491_520u32.to_le_bytes());
    m.extend([0u8; 8]); // no frames in the fourth block
    m
}

/// `[16-byte header][metadata][jpeg]`.
fn dataset(jpeg: &[u8]) -> Vec<u8> {
    let meta = metadata();
    let meta_len = meta.len() as u32;
    let mut out = Vec::with_capacity(HEADER_LEN as usize + meta.len() + jpeg.len());
    out.extend((HEADER_LEN + meta_len).to_le_bytes()); // image offset
    out.extend((jpeg.len() as u32).to_le_bytes()); // image size
    out.extend(HEADER_LEN.to_le_bytes()); // metadata offset
    out.extend(meta_len.to_le_bytes()); // metadata size
    out.extend(meta);
    out.extend(jpeg);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarted_numbering_changes_bytes_not_size() {
        let first = capture_jpeg(1, 0);
        let again = capture_jpeg(1, 1);
        assert_eq!(first.len(), again.len());
        assert_ne!(first, again);
        image::load_from_memory_with_format(&again, image::ImageFormat::Jpeg).unwrap();
    }
}
