//! Receiving new shots: GetObjectInfo/GetObject on the captured still handle
//! (0xFFFFC001), saved into the chosen destination folder.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::ptp::codes::{
    HANDLE_CAPTURED_STILL, OP_GET_OBJECT, OP_GET_OBJECT_INFO, PROP_CAPTURED_READY,
};
use crate::ptp::parse_object_info;
use crate::sony::property::read_property;
use crate::PtpIpConnection;

/// Shortest time limit for one GetObject; a large RAW takes far longer than
/// the default command timeout.
const MIN_TRANSFER_TIMEOUT: Duration = Duration::from_secs(60);
/// Highest `-N` suffix tried when a file name is already taken.
const MAX_SUFFIX: u32 = 999;
/// Bit 15 of 0xD215: shots are waiting; the low bits count them.
const READY_FLAG: u16 = 0x8000;

/// How the destination folder is laid out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DestinationMode {
    /// Every shot goes straight into the base folder.
    CustomFolder,
    /// A folder Lightroom auto-imports from; laid out like `CustomFolder`.
    LightroomWatched,
    /// One `YYYY-MM-DD` subfolder of the base folder per day.
    FolderPerDate,
}

/// Where received shots are saved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Destination {
    pub base: PathBuf,
    pub mode: DestinationMode,
}

impl Destination {
    /// The folder a shot taken on `now` goes into.
    pub fn target_dir(&self, now: NaiveDate) -> PathBuf {
        match self.mode {
            DestinationMode::CustomFolder | DestinationMode::LightroomWatched => self.base.clone(),
            DestinationMode::FolderPerDate => self.base.join(now.format("%Y-%m-%d").to_string()),
        }
    }

    /// Creates the target folder if needed and checks a file can be written
    /// there; returns the folder.
    pub fn ensure_writable(&self, now: NaiveDate) -> Result<PathBuf, Error> {
        let dir = self.target_dir(now);
        std::fs::create_dir_all(&dir)?;
        let probe = dir.join(".rimix-write-test");
        std::fs::write(&probe, b"")?;
        let _ = std::fs::remove_file(&probe);
        Ok(dir)
    }
}

/// A shot saved to disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Received {
    pub path: PathBuf,
    pub bytes: u64,
    /// The saved file's name, including any `-N` suffix.
    pub filename: String,
}

/// How many shots the camera holds ready to fetch (property 0xD215).
pub async fn captured_ready(conn: &mut PtpIpConnection) -> Result<u16, Error> {
    let prop = read_property(conn, PROP_CAPTURED_READY).await?;
    let raw = prop
        .current
        .as_u64()
        .and_then(|v| u16::try_from(v).ok())
        .ok_or_else(|| {
            Error::Protocol(format!(
                "captured-ready value {:?} is not a u16",
                prop.current
            ))
        })?;
    Ok(if raw & READY_FLAG != 0 {
        raw & !READY_FLAG
    } else {
        0
    })
}

/// Fetches the oldest waiting shot into `dest`.
///
/// The shot is written to `.rimix-<filename>.part` and renamed once complete,
/// so watchers such as Lightroom never see a partial file. A name already in
/// use gets a `-1`, `-2`, ... suffix. On any error the partial file is removed.
/// `on_progress(received, total)` is called before and after the transfer.
pub async fn receive_one(
    conn: &mut PtpIpConnection,
    dest: &Destination,
    on_progress: &mut (dyn FnMut(u64, u64) + Send),
) -> Result<Received, Error> {
    let dir = dest.ensure_writable(chrono::Local::now().date_naive())?;
    let (info, _) = conn
        .command_data_in(OP_GET_OBJECT_INFO, &[HANDLE_CAPTURED_STILL])
        .await?;
    let info = parse_object_info(&info)?;
    let filename = check_filename(&info.filename)?;
    on_progress(0, u64::from(info.compressed_size));

    let part = dir.join(format!(".rimix-{filename}.part"));
    let limit = conn.timeout().max(MIN_TRANSFER_TIMEOUT);
    let result = async {
        let (data, _) = conn
            .command_data_in_with_timeout(OP_GET_OBJECT, &[HANDLE_CAPTURED_STILL], limit)
            .await?;
        std::fs::write(&part, &data)?;
        let (path, filename) = free_name(&dir, filename)?;
        std::fs::rename(&part, &path)?;
        Ok(Received {
            path,
            bytes: data.len() as u64,
            filename,
        })
    }
    .await;
    match result {
        Ok(received) => {
            on_progress(received.bytes, received.bytes);
            Ok(received)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

/// Refuses camera-supplied names that would escape the target folder.
fn check_filename(name: &str) -> Result<&str, Error> {
    let plain =
        !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':', '\0']);
    if plain {
        Ok(name)
    } else {
        Err(Error::Protocol(format!(
            "camera sent an unusable file name {name:?}"
        )))
    }
}

/// `dir/name`, or `dir/<stem>-N.<ext>` with the first free N up to 999.
fn free_name(dir: &Path, name: &str) -> Result<(PathBuf, String), Error> {
    let path = dir.join(name);
    if !path.exists() {
        return Ok((path, name.to_string()));
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (name, None),
    };
    for n in 1..=MAX_SUFFIX {
        let candidate = match ext {
            Some(ext) => format!("{stem}-{n}.{ext}"),
            None => format!("{stem}-{n}"),
        };
        let path = dir.join(&candidate);
        if !path.exists() {
            return Ok((path, candidate));
        }
    }
    Err(Error::Io(std::io::Error::new(
        ErrorKind::AlreadyExists,
        format!("every name from {name} to suffix -{MAX_SUFFIX} is taken"),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_name_suffixes_before_the_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, name) = free_name(tmp.path(), "DSC00001.ARW").unwrap();
        assert_eq!(name, "DSC00001.ARW");
        std::fs::write(tmp.path().join("DSC00001.ARW"), b"").unwrap();
        let (path, name) = free_name(tmp.path(), "DSC00001.ARW").unwrap();
        assert_eq!(name, "DSC00001-1.ARW");
        assert_eq!(path, tmp.path().join("DSC00001-1.ARW"));
        std::fs::write(tmp.path().join("NOEXT"), b"").unwrap();
        assert_eq!(free_name(tmp.path(), "NOEXT").unwrap().1, "NOEXT-1");
    }

    #[test]
    fn free_name_gives_up_after_999() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("A.JPG"), b"").unwrap();
        for n in 1..=MAX_SUFFIX {
            std::fs::write(tmp.path().join(format!("A-{n}.JPG")), b"").unwrap();
        }
        let err = free_name(tmp.path(), "A.JPG").unwrap_err();
        assert!(
            matches!(&err, Error::Io(e) if e.kind() == ErrorKind::AlreadyExists),
            "{err:?}"
        );
    }

    #[test]
    fn path_like_filenames_are_refused() {
        for bad in ["", ".", "..", "../x.JPG", "a/b.JPG", "a\\b.JPG", "C:x.JPG"] {
            assert!(check_filename(bad).is_err(), "{bad:?}");
        }
        assert_eq!(check_filename("DSC00001.JPG").unwrap(), "DSC00001.JPG");
    }
}
