//! The app's log: a daily file `rimix.log` under `<config>/Rimix/logs`, at
//! `info` unless `RIMIX_LOG` says otherwise, and stderr too in debug builds.

use std::path::{Path, PathBuf};

use tracing::Subscriber;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

/// What the Wi-Fi logging uses to keep the password out of the log.
pub use rimix_wifi::redact;

/// How many daily files to keep.
const KEEP_DAYS: usize = 7;

/// Holds the file writer open; dropping it flushes and stops the log.
pub struct LogGuard(pub Option<WorkerGuard>);

/// The logs folder inside the config folder.
pub fn log_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("logs")
}

fn filter() -> EnvFilter {
    EnvFilter::try_from_env("RIMIX_LOG").unwrap_or_else(|_| EnvFilter::new("info"))
}

/// The daily file under `dir`; an error, never a panic, when it cannot be made.
fn appender(dir: &Path) -> std::io::Result<RollingFileAppender> {
    std::fs::create_dir_all(dir)?;
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("rimix.log")
        .max_log_files(KEEP_DAYS)
        .build(dir)
        .map_err(std::io::Error::other)
}

/// The file layer, plus stderr when `stderr` is set.
fn subscriber(file: NonBlocking, stderr: bool) -> impl Subscriber + Send + Sync {
    let stderr = stderr.then(|| fmt::layer().with_writer(std::io::stderr).boxed());
    tracing_subscriber::registry()
        .with(filter())
        .with(fmt::layer().with_writer(file).with_ansi(false))
        .with(stderr)
}

/// Installs the subscriber. Keep the guard alive for as long as the app runs.
pub fn init(dir: &Path) -> std::io::Result<WorkerGuard> {
    let (file, guard) = tracing_appender::non_blocking(appender(dir)?);
    subscriber(file, cfg!(debug_assertions) && !cfg!(test))
        .try_init()
        .map_err(std::io::Error::other)?;
    Ok(guard)
}

/// Logs to stderr only, for when the log folder cannot be made.
pub fn init_stderr() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter())
        .with_writer(std::io::stderr)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_the_log_file_in_the_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = log_dir(tmp.path());
        let (file, guard) = tracing_appender::non_blocking(appender(&dir).unwrap());
        tracing::subscriber::with_default(subscriber(file, false), || {
            tracing::info!(cmd = %redact(&["networksetup", "x", "secret"], &[2]), "networksetup");
        });
        drop(guard);
        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(files.len(), 1);
        let name = files[0].file_name().into_string().unwrap();
        assert!(name.starts_with("rimix.log"), "{name}");
        let text = std::fs::read_to_string(files[0].path()).unwrap();
        assert!(text.contains("networksetup x [password]"), "{text}");
        assert!(!text.contains("secret"), "{text}");
    }

    #[test]
    fn a_file_in_place_of_the_folder_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let not_a_dir = tmp.path().join("logs");
        std::fs::write(&not_a_dir, "").unwrap();
        assert!(init(&not_a_dir).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn an_unwritable_folder_is_an_error_not_a_panic() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("logs");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = init(&dir);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
    }
}
