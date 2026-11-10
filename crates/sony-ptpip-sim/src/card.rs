//! The simulator's memory card: a flat list of files whose bytes are
//! generated on the fly (byte `i` of handle `h` is `(i * 31 + h) as u8`), so
//! even large clips cost no memory and tests can recompute them.

use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use sony_ptpip::ptp::codes::{FMT_JPEG, FMT_MP4, FMT_SONY_RAW};
use sony_ptpip::ptp::{encode_object_info, ObjectInfo};

/// The one storage the simulated camera has.
pub const STORAGE_ID: u32 = 0x0001_0001;
/// The handle of the first card file; the rest follow in order.
const FIRST_HANDLE: u32 = 0x0000_0101;
const MIB: u64 = 1 << 20;

/// One file on the simulated card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimCardFile {
    pub filename: String,
    pub bytes: u64,
    pub format: u16,
    pub captured: NaiveDateTime,
}

/// `DSC00010.JPG` (2 MiB), `DSC00010.ARW` (24 MiB) and `C0001.MP4` (64 MiB).
pub fn default_card() -> Vec<SimCardFile> {
    let at = |h, m| {
        NaiveDate::from_ymd_opt(2026, 11, 10)
            .and_then(|d| d.and_hms_opt(h, m, 0))
            .expect("valid date")
    };
    vec![
        SimCardFile {
            filename: "DSC00010.JPG".into(),
            bytes: 2 * MIB,
            format: FMT_JPEG,
            captured: at(10, 15),
        },
        SimCardFile {
            filename: "DSC00010.ARW".into(),
            bytes: 24 * MIB,
            format: FMT_SONY_RAW,
            captured: at(10, 15),
        },
        SimCardFile {
            filename: "C0001.MP4".into(),
            bytes: 64 * MIB,
            format: FMT_MP4,
            captured: at(10, 20),
        },
    ]
}

/// The file behind `handle`, if any.
pub fn file(card: &[SimCardFile], handle: u32) -> Option<&SimCardFile> {
    let index = handle.checked_sub(FIRST_HANDLE)?;
    card.get(usize::try_from(index).ok()?)
}

/// The GetStorageIDs dataset.
pub fn storage_ids() -> Vec<u8> {
    u32_array(&[STORAGE_ID])
}

/// The GetObjectHandles dataset: every card file.
pub fn handles(card: &[SimCardFile]) -> Vec<u8> {
    let handles: Vec<u32> = (0..card.len() as u32).map(|i| FIRST_HANDLE + i).collect();
    u32_array(&handles)
}

/// The GetObjectInfo dataset of a card file.
pub fn object_info(f: &SimCardFile) -> Vec<u8> {
    encode_object_info(&ObjectInfo {
        storage_id: STORAGE_ID,
        object_format: f.format,
        compressed_size: u32::try_from(f.bytes).unwrap_or(u32::MAX),
        filename: f.filename.clone(),
        capture_date: f.captured.format("%Y%m%dT%H%M%S").to_string(),
    })
}

/// Bytes `offset..offset + len` of the file `handle`.
pub fn bytes(handle: u32, offset: u64, len: usize) -> Vec<u8> {
    (offset..offset + len as u64)
        .map(|i| i.wrapping_mul(31).wrapping_add(u64::from(handle)) as u8)
        .collect()
}

fn u32_array(values: &[u32]) -> Vec<u8> {
    let mut out = (values.len() as u32).to_le_bytes().to_vec();
    for v in values {
        out.extend(v.to_le_bytes());
    }
    out
}
