//! Browsing the memory card with the standard PTP object operations:
//! GetStorageIDs, GetObjectHandles and GetObjectInfo to list it, and
//! GetPartialObject to download a file in chunks, resuming after a drop.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::ptp::codes::{
    FMT_ASSOCIATION, FMT_JPEG, FMT_MP4, FMT_SONY_RAW, OP_GET_OBJECT_HANDLES, OP_GET_OBJECT_INFO,
    OP_GET_PARTIAL_OBJECT, OP_GET_STORAGE_IDS,
};
use crate::ptp::parse_object_info;
use crate::sony::capture::{check_filename, place, TRANSFER_IDLE};
use crate::PtpIpConnection;

/// How many bytes one GetPartialObject asks for.
pub const CARD_CHUNK: u32 = 1 << 20;
/// GetObjectHandles parent meaning "the root of the storage".
const ROOT: u32 = 0xFFFF_FFFF;
/// GetObjectHandles format meaning "every format".
const ALL_FORMATS: u32 = 0;

/// What a file on the card holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CardKind {
    Still,
    Raw,
    Video,
    Other,
}

impl CardKind {
    /// The kind from the PTP format code, or from the file extension when
    /// the code is not one Rimix knows.
    pub fn of(format: u16, filename: &str) -> CardKind {
        match format {
            FMT_JPEG => return CardKind::Still,
            FMT_SONY_RAW => return CardKind::Raw,
            FMT_MP4 => return CardKind::Video,
            _ => {}
        }
        let ext = filename
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "jpg" | "jpeg" | "hif" | "heif" | "heic" => CardKind::Still,
            "arw" => CardKind::Raw,
            "mp4" | "mov" | "mts" | "m2ts" | "xavc" => CardKind::Video,
            _ => CardKind::Other,
        }
    }
}

/// One file on the camera's card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardObject {
    pub handle: u32,
    pub storage: u32,
    pub filename: String,
    pub bytes: u64,
    pub format: u16,
    pub kind: CardKind,
    pub captured: Option<NaiveDateTime>,
}

/// Parses a PTP date-time string, `YYYYMMDDThhmmss` with an optional
/// fraction (`.s`) and zone (`Z`, `+hhmm`, `-hhmm`), which are ignored.
pub fn parse_ptp_date(s: &str) -> Option<NaiveDateTime> {
    let head = s.get(..15)?;
    let rest = &s[15..];
    let tail_ok = rest.is_empty()
        || rest.starts_with(['.', 'Z', '+', '-'])
            && rest[1..]
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | 'Z' | '.'));
    if !tail_ok {
        return None;
    }
    NaiveDateTime::parse_from_str(head, "%Y%m%dT%H%M%S").ok()
}

/// Decodes a PTP `u32` array: a `u32` count, then the elements.
fn u32_array(data: &[u8]) -> Result<Vec<u32>, Error> {
    let word = |i: usize| -> Option<u32> {
        let b = data.get(i * 4..i * 4 + 4)?;
        Some(u32::from_le_bytes(b.try_into().ok()?))
    };
    let bad = || Error::Protocol(format!("bad PTP u32 array of {} bytes", data.len()));
    let count = word(0).ok_or_else(bad)? as usize;
    if data.len() != 4 + count.checked_mul(4).ok_or_else(bad)? {
        return Err(bad());
    }
    Ok((1..=count).filter_map(word).collect())
}

/// The camera's storage ids (GetStorageIDs).
pub async fn list_storages(conn: &mut PtpIpConnection) -> Result<Vec<u32>, Error> {
    let (data, _) = conn.command_data_in(OP_GET_STORAGE_IDS, &[]).await?;
    u32_array(&data)
}

/// The handles of every object at the root of `storage` (GetObjectHandles).
pub async fn object_handles(conn: &mut PtpIpConnection, storage: u32) -> Result<Vec<u32>, Error> {
    let (data, _) = conn
        .command_data_in(OP_GET_OBJECT_HANDLES, &[storage, ALL_FORMATS, ROOT])
        .await?;
    u32_array(&data)
}

/// The file behind `handle` (GetObjectInfo), or `None` for a folder
/// (association).
pub async fn object(conn: &mut PtpIpConnection, handle: u32) -> Result<Option<CardObject>, Error> {
    let (data, _) = conn.command_data_in(OP_GET_OBJECT_INFO, &[handle]).await?;
    let info = parse_object_info(&data)?;
    if info.object_format == FMT_ASSOCIATION {
        return Ok(None);
    }
    Ok(Some(CardObject {
        handle,
        storage: info.storage_id,
        kind: CardKind::of(info.object_format, &info.filename),
        bytes: u64::from(info.compressed_size),
        format: info.object_format,
        captured: parse_ptp_date(&info.capture_date),
        filename: info.filename,
    }))
}

/// Every file at the root of `storage`, in the camera's order; folders are
/// skipped.
pub async fn list_objects(
    conn: &mut PtpIpConnection,
    storage: u32,
) -> Result<Vec<CardObject>, Error> {
    let mut out = Vec::new();
    for handle in object_handles(conn, storage).await? {
        out.extend(object(conn, handle).await?);
    }
    Ok(out)
}

/// Partial files being written in this process, so two downloads never
/// append to the same one.
static ACTIVE: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

/// A download in progress: `dir/.rimix-<name>.part` holds the bytes
/// received so far. Fetch chunks with [`PartialDownload::next_chunk`] until
/// [`PartialDownload::done`], then [`PartialDownload::finish`].
///
/// Dropping it unfinished keeps the partial file, so a later download of
/// the same file resumes where this one stopped.
#[derive(Debug)]
pub struct PartialDownload {
    handle: u32,
    dir: PathBuf,
    name: String,
    part: PathBuf,
    received: u64,
    total: u64,
}

impl PartialDownload {
    /// Prepares downloading `obj` into `dest_dir`, which is created if
    /// needed. An existing partial file is resumed from its length, or
    /// discarded if it is longer than the file.
    pub fn begin(obj: &CardObject, dest_dir: &Path) -> Result<PartialDownload, Error> {
        let name = check_filename(&obj.filename)?.to_string();
        std::fs::create_dir_all(dest_dir)?;
        let part = dest_dir.join(format!(".rimix-{name}.part"));
        {
            let mut active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
            if !active.get_or_insert_with(HashSet::new).insert(part.clone()) {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("{name} is already downloading into {}", dest_dir.display()),
                )));
            }
        }
        // From here on, Drop releases the claim.
        let mut dl = PartialDownload {
            handle: obj.handle,
            dir: dest_dir.to_path_buf(),
            name,
            part,
            received: 0,
            total: obj.bytes,
        };
        let have = match std::fs::metadata(&dl.part) {
            Ok(m) => m.len(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
            Err(e) => return Err(e.into()),
        };
        if have > dl.total {
            tracing::info!(part = %dl.part.display(), have, total = dl.total, "partial file longer than the card file; starting over");
            std::fs::remove_file(&dl.part)?;
        } else {
            dl.received = have;
        }
        // Make sure the partial file exists, also for an empty card file.
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&dl.part)?;
        Ok(dl)
    }

    /// Bytes received so far, including any resumed from an earlier try.
    pub fn received(&self) -> u64 {
        self.received
    }

    /// The file's size on the card.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// True once every byte is in the partial file.
    pub fn done(&self) -> bool {
        self.received >= self.total
    }

    /// Fetches the next chunk of at most `chunk` bytes with
    /// GetPartialObject and appends it to the partial file.
    pub async fn next_chunk(
        &mut self,
        conn: &mut PtpIpConnection,
        chunk: u32,
    ) -> Result<(), Error> {
        if self.done() {
            return Ok(());
        }
        let offset = u32::try_from(self.received)
            .map_err(|_| Error::Unsupported("card files above 4 GiB"))?;
        let want = u32::try_from((self.total - self.received).min(u64::from(chunk.max(1))))
            .unwrap_or(chunk);
        let idle = conn.timeout().max(TRANSFER_IDLE);
        let (data, params) = conn
            .command_data_in_streaming(
                OP_GET_PARTIAL_OBJECT,
                &[self.handle, offset, want],
                idle,
                &mut |_| {},
            )
            .await?;
        if let Some(&sent) = params.first() {
            if u64::from(sent) != data.len() as u64 {
                return Err(Error::Protocol(format!(
                    "GetPartialObject said {sent} bytes but sent {}",
                    data.len()
                )));
            }
        }
        if data.is_empty() || data.len() as u64 > u64::from(want) {
            return Err(Error::Protocol(format!(
                "GetPartialObject at {offset} sent {} bytes for {want} asked",
                data.len()
            )));
        }
        let mut file = std::fs::OpenOptions::new().append(true).open(&self.part)?;
        file.write_all(&data)?;
        file.flush()?;
        self.received += data.len() as u64;
        Ok(())
    }

    /// Renames the complete partial file to a free name (`-N` suffix on a
    /// clash); returns the final path. Refuses while bytes are missing or
    /// the partial file's length differs from the card file's.
    pub fn finish(self) -> Result<PathBuf, Error> {
        let len = std::fs::metadata(&self.part)?.len();
        if !self.done() || len != self.total {
            return Err(Error::Protocol(format!(
                "{} is incomplete ({len} of {} bytes)",
                self.name, self.total
            )));
        }
        let (path, _) = place(&self.part, &self.dir, &self.name)?;
        Ok(path)
    }
}

impl Drop for PartialDownload {
    fn drop(&mut self) {
        let mut active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(set) = active.as_mut() {
            set.remove(&self.part);
        }
    }
}

/// Downloads `obj` into `dest_dir` in chunks of `chunk` bytes, resuming
/// from `.rimix-<name>.part` if an earlier try left one, and renames it into
/// place once complete. `progress(received, total)` is called after every
/// chunk. On error the partial file is kept for the next try.
pub async fn download(
    conn: &mut PtpIpConnection,
    obj: &CardObject,
    dest_dir: &Path,
    chunk: u32,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf, Error> {
    let mut dl = PartialDownload::begin(obj, dest_dir)?;
    while !dl.done() {
        dl.next_chunk(conn, chunk).await?;
        progress(dl.received(), dl.total());
    }
    dl.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn ptp_dates_parse_with_or_without_fraction_and_zone() {
        let want = NaiveDate::from_ymd_opt(2026, 11, 10)
            .unwrap()
            .and_hms_opt(10, 15, 0)
            .unwrap();
        for s in [
            "20261110T101500",
            "20261110T101500.0",
            "20261110T101500Z",
            "20261110T101500+0100",
            "20261110T101500.5-0800",
        ] {
            assert_eq!(parse_ptp_date(s), Some(want), "{s}");
        }
        for s in [
            "",
            "2026",
            "20261110 101500",
            "20261310T101500",
            "20261110T101500x",
        ] {
            assert_eq!(parse_ptp_date(s), None, "{s}");
        }
    }

    #[test]
    fn kinds_come_from_the_format_then_the_extension() {
        assert_eq!(CardKind::of(FMT_JPEG, "X.BIN"), CardKind::Still);
        assert_eq!(CardKind::of(FMT_SONY_RAW, "X"), CardKind::Raw);
        assert_eq!(CardKind::of(FMT_MP4, "X"), CardKind::Video);
        assert_eq!(CardKind::of(0x3000, "DSC1.HIF"), CardKind::Still);
        assert_eq!(CardKind::of(0x3000, "DSC1.arw"), CardKind::Raw);
        assert_eq!(CardKind::of(0x3000, "C1.MP4"), CardKind::Video);
        assert_eq!(CardKind::of(0x3000, "INDEX.XML"), CardKind::Other);
    }

    #[test]
    fn u32_arrays_decode_and_reject_bad_lengths() {
        let mut b = 2u32.to_le_bytes().to_vec();
        b.extend(7u32.to_le_bytes());
        b.extend(9u32.to_le_bytes());
        assert_eq!(u32_array(&b).unwrap(), vec![7, 9]);
        assert!(u32_array(&b[..8]).is_err());
        assert!(u32_array(&[]).is_err());
    }
}
