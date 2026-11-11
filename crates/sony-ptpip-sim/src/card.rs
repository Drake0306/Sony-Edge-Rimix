//! The simulator's memory card: files in folders, as a real Sony card holds
//! them (`DCIM/100MSDCF`, `PRIVATE/M4ROOT/CLIP`). Folders are served as
//! associations and GetObjectHandles answers per parent. File bytes are
//! generated on the fly (byte `i` of handle `h` is `(i * 31 + h) as u8`), so
//! even large clips cost no memory and tests can recompute them.

use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use sony_ptpip::ptp::codes::{FMT_ASSOCIATION, FMT_JPEG, FMT_MP4, FMT_SONY_RAW};
use sony_ptpip::ptp::{encode_object_info, ObjectInfo};

/// The one storage the simulated camera has.
pub const STORAGE_ID: u32 = 0x0001_0001;
/// The handle of the first card file; the rest follow in order.
const FIRST_HANDLE: u32 = 0x0000_0101;
/// The handle of the first folder; the rest follow in order.
const FIRST_FOLDER: u32 = 0x0010_0001;
/// GetObjectHandles parent meaning "the root of the storage".
pub const ROOT: u32 = 0xFFFF_FFFF;
/// GetObjectHandles parent meaning "every object, whatever its folder".
pub const ANY_PARENT: u32 = 0;
const MIB: u64 = 1 << 20;

/// One file on the simulated card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimCardFile {
    /// The folder holding the file, `/`-separated from the root
    /// (`DCIM/100MSDCF`); empty for the root itself.
    #[serde(default)]
    pub folder: String,
    pub filename: String,
    pub bytes: u64,
    pub format: u16,
    pub captured: NaiveDateTime,
}

/// `DCIM/100MSDCF/DSC00010.JPG` (2 MiB), `DCIM/100MSDCF/DSC00010.ARW`
/// (24 MiB) and `PRIVATE/M4ROOT/CLIP/C0001.MP4` (64 MiB).
pub fn default_card() -> Vec<SimCardFile> {
    let at = |h, m| {
        NaiveDate::from_ymd_opt(2026, 11, 10)
            .and_then(|d| d.and_hms_opt(h, m, 0))
            .expect("valid date")
    };
    vec![
        SimCardFile {
            folder: "DCIM/100MSDCF".into(),
            filename: "DSC00010.JPG".into(),
            bytes: 2 * MIB,
            format: FMT_JPEG,
            captured: at(10, 15),
        },
        SimCardFile {
            folder: "DCIM/100MSDCF".into(),
            filename: "DSC00010.ARW".into(),
            bytes: 24 * MIB,
            format: FMT_SONY_RAW,
            captured: at(10, 15),
        },
        SimCardFile {
            folder: "PRIVATE/M4ROOT/CLIP".into(),
            filename: "C0001.MP4".into(),
            bytes: 64 * MIB,
            format: FMT_MP4,
            captured: at(10, 20),
        },
    ]
}

/// The clip a finished movie recording leaves in `PRIVATE/M4ROOT/CLIP`:
/// `C000<n>.MP4`, 3 MiB.
pub fn new_clip(n: u32) -> SimCardFile {
    SimCardFile {
        folder: "PRIVATE/M4ROOT/CLIP".into(),
        filename: format!("C{n:04}.MP4"),
        bytes: 3 * MIB,
        format: FMT_MP4,
        captured: chrono::Local::now().naive_local(),
    }
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

/// Every folder on the card as a `/`-separated path, each after its parent,
/// in the order the files first name them.
pub fn folders(card: &[SimCardFile]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in card {
        let mut path = String::new();
        for part in f.folder.split('/').filter(|p| !p.is_empty()) {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            if !out.contains(&path) {
                out.push(path.clone());
            }
        }
    }
    out
}

/// The folder path behind `handle`, if any.
pub fn folder(card: &[SimCardFile], handle: u32) -> Option<String> {
    let index = handle.checked_sub(FIRST_FOLDER)?;
    folders(card).into_iter().nth(usize::try_from(index).ok()?)
}

/// The folder a `/`-separated path sits in; empty for the root.
fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(p, _)| p).unwrap_or("")
}

/// The handles under `parent` (a folder handle, [`ROOT`] or
/// [`ANY_PARENT`]): its folders first, then its files, each in card order.
/// `None` when `parent` is not a folder.
pub fn children(card: &[SimCardFile], parent: u32) -> Option<Vec<u32>> {
    let all = folders(card);
    let within: Option<&str> = match parent {
        ANY_PARENT => None,
        ROOT => Some(""),
        h => {
            let index = usize::try_from(h.checked_sub(FIRST_FOLDER)?).ok()?;
            Some(all.get(index)?.as_str())
        }
    };
    let inside = |folder: &str| within.is_none_or(|w| w == folder);
    let mut out = Vec::new();
    for (i, path) in all.iter().enumerate() {
        if inside(parent_of(path)) {
            out.push(FIRST_FOLDER + i as u32);
        }
    }
    for (i, f) in card.iter().enumerate() {
        if inside(f.folder.trim_matches('/')) {
            out.push(FIRST_HANDLE + i as u32);
        }
    }
    Some(out)
}

/// A name for `handle` in the simulator's log.
pub fn name(card: &[SimCardFile], handle: u32) -> String {
    if let Some(f) = file(card, handle) {
        return f.filename.clone();
    }
    match folder(card, handle) {
        Some(path) => format!("{}/", path.rsplit('/').next().unwrap_or("")),
        None => format!("{handle:#x}"),
    }
}

/// The GetObjectHandles dataset for `handles`.
pub fn handles(handles: &[u32]) -> Vec<u8> {
    u32_array(handles)
}

/// The GetObjectInfo dataset of a folder.
pub fn folder_info(path: &str) -> Vec<u8> {
    encode_object_info(&ObjectInfo {
        storage_id: STORAGE_ID,
        object_format: FMT_ASSOCIATION,
        compressed_size: 0,
        filename: path.rsplit('/').next().unwrap_or("").to_string(),
        capture_date: String::new(),
    })
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
