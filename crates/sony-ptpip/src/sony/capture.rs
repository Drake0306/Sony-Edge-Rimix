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
    PROP_IMAGE_SIZE_TO_HOST, PROP_STILL_DESTINATION,
};
use crate::ptp::{parse_object_info, PtpValue};
use crate::quirks::Quirks;
use crate::sony::control::set_value;
use crate::sony::property::{Enabled, Form, PropertyDesc};
use crate::PtpIpConnection;

/// Longest silence from the camera while a shot downloads; the download as
/// a whole has no limit, since a large RAW on a slow link takes long.
pub const TRANSFER_IDLE: Duration = Duration::from_secs(10);
/// Highest `-N` suffix tried when a file name is already taken.
const MAX_SUFFIX: u32 = 999;
/// Bit 15 of 0xD215: shots are waiting; the low bits count them.
const READY_FLAG: u16 = 0x8000;

/// Bit of 0xD222 (still image save destination) for the computer.
pub const DEST_COMPUTER: u64 = 0x0001;
/// Bit of 0xD222 for the camera's card.
const DEST_CARD: u64 = 0x0010;
/// 0xD268 (size of image sent to the computer): the original file.
const HOST_SIZE_ORIGINAL: u64 = 1;

/// The save destination to write so new pictures reach the computer as
/// well as the card, or `None` when nothing should change: the computer
/// already gets them, or the camera offers no setting that keeps the card
/// copy too (the card copy is never given up for this).
pub fn host_delivery_target(dest: &PropertyDesc) -> Option<PtpValue> {
    delivery_target(dest, true)
}

/// The save destination to write for pictures to go to the computer as
/// well as the card (`to_computer`), or to the card alone, or `None` when
/// nothing should change: it is so already, or the camera lists no such
/// value. Either way the card copy is kept.
pub fn delivery_target(dest: &PropertyDesc, to_computer: bool) -> Option<PtpValue> {
    let current = dest.current.as_u64()?;
    if (current & DEST_COMPUTER != 0) == to_computer && current & DEST_CARD != 0 {
        return None;
    }
    // "Computer only" already reaches the computer; it is left as it is.
    if to_computer && current & DEST_COMPUTER != 0 {
        return None;
    }
    let Form::Enum { settable, .. } = &dest.form else {
        return None;
    };
    let wanted = if to_computer {
        DEST_COMPUTER | DEST_CARD
    } else {
        DEST_CARD
    };
    let both = DEST_COMPUTER | DEST_CARD;
    settable
        .iter()
        .find(|v| v.as_u64().is_some_and(|v| v & both == wanted))
        .cloned()
}

/// Makes the camera hand new pictures to the computer, in full size, when
/// it is set to keep them on its card only (as the ZV-E10 arrives): sets
/// the save destination (0xD222) to computer and card, and the size sent
/// to the computer (0xD268) to the original. Returns whether anything was
/// written, so the caller can read the properties again.
///
/// A camera that refuses is left as it is, with a warning: the session is
/// still good for everything else. Only a lost connection is an error.
pub async fn ensure_host_delivery(
    conn: &mut PtpIpConnection,
    props: &[PropertyDesc],
    quirks: &Quirks,
) -> Result<bool, Error> {
    ensure_delivery(conn, props, quirks, true).await
}

/// [`ensure_host_delivery`] in either direction: with `to_computer` false
/// the camera is put back to saving on its card alone, so it does not
/// hold pictures for a computer that will not fetch them.
pub async fn ensure_delivery(
    conn: &mut PtpIpConnection,
    props: &[PropertyDesc],
    quirks: &Quirks,
    to_computer: bool,
) -> Result<bool, Error> {
    let find = |code| props.iter().find(|p: &&PropertyDesc| p.code == code);
    let mut wanted: Vec<(&PropertyDesc, PtpValue, &str)> = Vec::new();
    match find(PROP_STILL_DESTINATION) {
        Some(dest) => match delivery_target(dest, to_computer) {
            Some(to) => wanted.push((dest, to, "still image save destination")),
            None if to_computer && dest.current.as_u64().is_some_and(|v| v & DEST_COMPUTER == 0) => {
                tracing::warn!(
                    current = ?dest.current,
                    "the camera saves pictures to its card only and offers no setting that also sends them here; set Still Img. Save Dest. to PC+Camera on the camera"
                );
            }
            None => {}
        },
        None => tracing::info!(
            "the camera does not report its save destination (0xD222); pictures arrive only if it is set to send them"
        ),
    }
    if let (true, Some(size)) = (to_computer, find(PROP_IMAGE_SIZE_TO_HOST)) {
        let original = PtpValue::U8(HOST_SIZE_ORIGINAL as u8);
        if size
            .current
            .as_u64()
            .is_some_and(|v| v != HOST_SIZE_ORIGINAL)
            && size.accepts(&original)
        {
            wanted.push((size, original, "size of pictures sent to the computer"));
        }
    }
    let mut written = false;
    for (desc, to, what) in wanted {
        if !desc.writable || desc.enabled != Enabled::On {
            tracing::warn!(code = %format_args!("{:#06x}", desc.code), "the camera will not let the {what} be changed now");
            continue;
        }
        match set_value(conn, desc, &to, quirks).await {
            Ok(()) => {
                // The camera answering OK is not yet the change: the real
                // ZV-E10 said OK and reported the old value afterwards.
                tracing::info!(from = ?desc.current, to = ?to, "asked the camera to change its {what}");
                written = true;
            }
            Err(e) if matches!(e, Error::Disconnected) || conn.is_disconnected() => return Err(e),
            Err(e) => tracing::warn!(error = %e, "the camera refused to change the {what}"),
        }
    }
    Ok(written)
}

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

/// How many shots the camera holds ready to fetch, from property 0xD215 in
/// a property table read with
/// [`read_all_props`](crate::sony::property::read_all_props). This talks to
/// no camera: like libgphoto2, the flag is taken from the 0x9209 table
/// rather than read on its own with 0x9251, which some bodies never answer.
/// A table without 0xD215 counts as nothing waiting.
pub fn captured_ready(props: &[PropertyDesc]) -> Result<u16, Error> {
    let Some(prop) = props.iter().find(|p| p.code == PROP_CAPTURED_READY) else {
        // Said once: without 0xD215 in the table, shots are never fetched.
        static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !props.is_empty() && !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            tracing::warn!(
                "the camera's property table has no 0xD215 (captured-ready); shots will not be received automatically"
            );
        }
        return Ok(0);
    };
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

    fn dest(current: u16, settable: &[u16]) -> PropertyDesc {
        PropertyDesc {
            code: crate::ptp::codes::PROP_STILL_DESTINATION,
            datatype: 0x0004,
            writable: true,
            enabled: crate::sony::property::Enabled::On,
            default: PtpValue::U16(current),
            current: PtpValue::U16(current),
            form: crate::sony::property::Form::Enum {
                settable: settable.iter().copied().map(PtpValue::U16).collect(),
                reportable: vec![],
            },
        }
    }

    #[test]
    fn card_only_becomes_computer_and_card() {
        let all = [0x0001, 0x0010, 0x0011];
        assert_eq!(
            host_delivery_target(&dest(0x0010, &all)),
            Some(PtpValue::U16(0x0011))
        );
        // Already reaching the computer: nothing to change.
        assert_eq!(host_delivery_target(&dest(0x0011, &all)), None);
        assert_eq!(host_delivery_target(&dest(0x0001, &all)), None);
    }

    #[test]
    fn switching_off_goes_back_to_the_card_alone() {
        let all = [0x0001, 0x0010, 0x0011];
        assert_eq!(
            delivery_target(&dest(0x0011, &all), false),
            Some(PtpValue::U16(0x0010))
        );
        assert_eq!(
            delivery_target(&dest(0x0001, &all), false),
            Some(PtpValue::U16(0x0010))
        );
        // Already the card alone, or no such choice: nothing to change.
        assert_eq!(delivery_target(&dest(0x0010, &all), false), None);
        assert_eq!(
            delivery_target(&dest(0x0011, &[0x0001, 0x0011]), false),
            None
        );
    }

    #[test]
    fn the_card_copy_is_never_given_up_to_reach_the_computer() {
        // Only "computer only" on offer: switching would stop the card copy.
        assert_eq!(host_delivery_target(&dest(0x0010, &[0x0001, 0x0010])), None);
        assert_eq!(host_delivery_target(&dest(0x0010, &[0x0010])), None);
    }

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
