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

/// Longest silence from the camera while a shot downloads; the download as
/// a whole has no limit, since a large RAW on a slow link takes long.
pub const TRANSFER_IDLE: Duration = Duration::from_secs(10);
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
    /// Where a downloaded shot goes when it cannot be written to the target
    /// folder; `None` means [`default_recovery_dir`].
    #[serde(default)]
    pub recovery_dir: Option<PathBuf>,
}

/// `<config folder>/Rimix/Recovered` (on macOS
/// `~/Library/Application Support/Rimix/Recovered`), or a `Rimix/Recovered`
/// folder in the temp folder where there is no config folder.
pub fn default_recovery_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "Rimix")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("Rimix"))
        .join("Recovered")
}

impl Destination {
    /// The folder a shot taken on `now` goes into.
    pub fn target_dir(&self, now: NaiveDate) -> PathBuf {
        match self.mode {
            DestinationMode::CustomFolder | DestinationMode::LightroomWatched => self.base.clone(),
            DestinationMode::FolderPerDate => self.base.join(now.format("%Y-%m-%d").to_string()),
        }
    }

    /// The folder for shots that cannot be saved in the target folder.
    pub fn recovery_dir(&self) -> PathBuf {
        self.recovery_dir
            .clone()
            .unwrap_or_else(default_recovery_dir)
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
    /// The camera's name for the file, before any `-N` suffix; progress
    /// events name the file this way.
    pub camera_filename: String,
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
/// The shot is written to `.rimix-<filename>.<n>.part` and renamed once complete,
/// so watchers such as Lightroom never see a partial file. A name already in
/// use gets a `-1`, `-2`, ... suffix. On any error the partial file is removed.
///
/// Once GetObject has succeeded the camera counts the shot as delivered, so
/// if it then cannot be written to the target folder (disk full, drive
/// unplugged, no permission, every name taken) it is written to
/// [`Destination::recovery_dir`] instead and [`Error::Recovered`] names
/// where it went.
///
/// `on_progress(received, total)` is called before the transfer, after
/// every packet of it, and once the file is saved. The transfer has no
/// overall time limit; it fails with [`Error::Timeout`] only when the camera
/// sends nothing for [`TRANSFER_IDLE`] (or the connection's timeout, if
/// longer).
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
    let camera_filename = check_filename(&info.filename)?;
    let total = u64::from(info.compressed_size);
    on_progress(0, total);

    let idle = conn.timeout().max(TRANSFER_IDLE);
    let (data, _) = conn
        .command_data_in_streaming(
            OP_GET_OBJECT,
            &[HANDLE_CAPTURED_STILL],
            idle,
            &mut |received| on_progress(received, total),
        )
        .await?;
    let bytes = data.len() as u64;
    match save(&dir, camera_filename, &data) {
        Ok((path, filename)) => {
            on_progress(bytes, bytes);
            Ok(Received {
                path,
                bytes,
                filename,
                camera_filename: camera_filename.to_string(),
            })
        }
        Err(e) => Err(recover(&dest.recovery_dir(), camera_filename, &data, e)),
    }
}

/// Held while a free name is picked and taken, so cameras in one process
/// saving the same name into the same folder never pick the same one.
static NAMING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Tells apart the partial files of saves running at the same time.
static PART_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Writes `data` to `dir/.rimix-<name>.<n>.part`, then renames it to a free
/// name; returns the path and name. The partial file is removed on error.
fn save(dir: &Path, name: &str, data: &[u8]) -> std::io::Result<(PathBuf, String)> {
    let seq = PART_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let part = dir.join(format!(".rimix-{name}.{}-{seq}.part", std::process::id()));
    let result = std::fs::write(&part, data).and_then(|()| place(&part, dir, name));
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// Moves the complete file `part` to a free name for `name` in `dir` (see
/// [`free_name`]), holding the naming lock, without ever replacing an
/// existing file; returns the path and name.
pub(crate) fn place(part: &Path, dir: &Path, name: &str) -> std::io::Result<(PathBuf, String)> {
    let _naming = NAMING.lock().unwrap_or_else(|e| e.into_inner());
    // The lock only covers this process's own savers; another saver in the
    // folder (the other receiver, another app) may take the name at any
    // moment. A hard link fails rather than replace an existing file, so
    // a taken name moves on to the next suffix and nothing is overwritten.
    for _ in 0..=MAX_SUFFIX + 1 {
        let (path, filename) = free_name(dir, name)?;
        match std::fs::hard_link(part, &path) {
            Ok(()) => {
                if let Err(e) = std::fs::remove_file(part) {
                    tracing::warn!(part = %part.display(), error = %e, "saved, but the partial file could not be removed");
                }
                return Ok((path, filename));
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => {
                // No hard links on this volume (exFAT, FAT, some network
                // shares): rename, which the lock guards against this
                // process's own savers.
                tracing::debug!(error = %e, "hard link refused; renaming instead");
                std::fs::rename(part, &path)?;
                return Ok((path, filename));
            }
        }
    }
    Err(std::io::Error::new(
        ErrorKind::AlreadyExists,
        format!("every name from {name} to suffix -{MAX_SUFFIX} is taken"),
    ))
}

/// Keeps a shot that could not be saved (`cause`) in `recovery`. Returns
/// [`Error::Recovered`], or, if that fails too, an I/O error naming both
/// failures.
fn recover(recovery: &Path, name: &str, data: &[u8], cause: std::io::Error) -> Error {
    let kept = std::fs::create_dir_all(recovery).and_then(|()| {
        let (path, _) = free_name(recovery, name)?;
        if let Err(e) = std::fs::write(&path, data) {
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
        Ok(path)
    });
    match kept {
        Ok(path) => {
            tracing::warn!(error = %cause, path = %path.display(), "shot kept in the recovery folder");
            Error::Recovered {
                path,
                source: cause,
            }
        }
        Err(e) => {
            tracing::error!(error = %cause, recovery_error = %e, "a received shot could not be kept");
            Error::Io(std::io::Error::new(
                cause.kind(),
                format!(
                    "{cause}; keeping it in {} failed too: {e}",
                    recovery.display()
                ),
            ))
        }
    }
}

/// Refuses camera-supplied names that would escape the target folder.
pub(crate) fn check_filename(name: &str) -> Result<&str, Error> {
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
pub(crate) fn free_name(dir: &Path, name: &str) -> std::io::Result<(PathBuf, String)> {
    // Taken means anything at the path, a dangling link included.
    let taken = |p: &Path| std::fs::symlink_metadata(p).is_ok();
    let path = dir.join(name);
    if !taken(&path) {
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
        if !taken(&path) {
            return Ok((path, candidate));
        }
    }
    Err(std::io::Error::new(
        ErrorKind::AlreadyExists,
        format!("every name from {name} to suffix -{MAX_SUFFIX} is taken"),
    ))
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
    fn place_never_replaces_a_file_and_leaves_no_part() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("DSC00001.JPG"), b"first").unwrap();
        let part = tmp.path().join(".rimix-DSC00001.JPG.part");
        std::fs::write(&part, b"second").unwrap();
        let (path, name) = place(&part, tmp.path(), "DSC00001.JPG").unwrap();
        assert_eq!(name, "DSC00001-1.JPG");
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert_eq!(
            std::fs::read(tmp.path().join("DSC00001.JPG")).unwrap(),
            b"first"
        );
        assert!(!part.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_link_counts_as_taken() {
        let tmp = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(tmp.path().join("gone"), tmp.path().join("A.JPG")).unwrap();
        assert_eq!(free_name(tmp.path(), "A.JPG").unwrap().1, "A-1.JPG");
    }

    #[test]
    fn free_name_gives_up_after_999() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("A.JPG"), b"").unwrap();
        for n in 1..=MAX_SUFFIX {
            std::fs::write(tmp.path().join(format!("A-{n}.JPG")), b"").unwrap();
        }
        let err = free_name(tmp.path(), "A.JPG").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::AlreadyExists, "{err:?}");
    }

    /// Two cameras in one app can save shots with the same name into the
    /// same folder at the same moment: each keeps its own file and bytes.
    #[test]
    fn concurrent_saves_of_one_name_keep_every_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        for round in 0..20 {
            let name = format!("DSC{round:05}.JPG");
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
            let saves: Vec<_> = (0..4u8)
                .map(|i| {
                    let (dir, name, barrier) = (dir.clone(), name.clone(), barrier.clone());
                    std::thread::spawn(move || {
                        let data = vec![i; 64 * 1024];
                        barrier.wait();
                        let (path, _) = save(&dir, &name, &data).unwrap();
                        (path, i)
                    })
                })
                .collect();
            let mut paths = Vec::new();
            for s in saves {
                let (path, i) = s.join().unwrap();
                assert_eq!(
                    std::fs::read(&path).unwrap(),
                    vec![i; 64 * 1024],
                    "{path:?}"
                );
                paths.push(path);
            }
            paths.sort();
            paths.dedup();
            assert_eq!(paths.len(), 4, "{paths:?}");
        }
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".part"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn path_like_filenames_are_refused() {
        for bad in ["", ".", "..", "../x.JPG", "a/b.JPG", "a\\b.JPG", "C:x.JPG"] {
            assert!(check_filename(bad).is_err(), "{bad:?}");
        }
        assert_eq!(check_filename("DSC00001.JPG").unwrap(), "DSC00001.JPG");
    }
}
