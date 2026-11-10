//! An SFTP server for the camera's own "FTP Transfer Func.": the camera
//! logs in with the one account the user set up and uploads stills and
//! proxies, which appear in the destination folder only once complete.
//!
//! The client's `/` is [`ReceiverConfig::destination`]. Each upload is
//! written to a hidden `.rimix-sftp-<name>.<pid>-<seq>.part` next to its
//! final place and renamed to a free name (`name`, `name-1`, ...) when the
//! client closes it; [`Receiver::start`]'s `on_file` callback then fires.

mod handler;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use russh::keys::{Algorithm, HashAlg, PrivateKey};
use russh::{server, MethodKind, MethodSet};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// What the receiver listens on, who may log in and where files go.
#[derive(Clone)]
pub struct ReceiverConfig {
    /// Port 0 picks a free port; see [`Receiver::local_addr`].
    pub bind: SocketAddr,
    pub user: String,
    /// Never logged.
    pub password: String,
    pub host_key: PrivateKey,
    /// The client's `/`; created if missing.
    pub destination: PathBuf,
}

impl std::fmt::Debug for ReceiverConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReceiverConfig")
            .field("bind", &self.bind)
            .field("user", &self.user)
            .field("password", &"<hidden>")
            .field("destination", &self.destination)
            .finish_non_exhaustive()
    }
}

/// A file the client finished uploading, already in its final place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceivedFile {
    /// Where it was saved; its name may carry a `-N` suffix.
    pub path: PathBuf,
    pub bytes: u64,
    /// The path the client wrote to, normalised (`/DSC00001.JPG`).
    pub remote_name: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("host key: {0}")]
    HostKey(String),
}

/// Bad logins on one connection before it is closed.
pub(crate) const MAX_FAILURES: u32 = 3;

/// What every session shares.
pub(crate) struct Shared {
    pub user: String,
    pub password: String,
    /// The destination as configured (made absolute): paths handed out
    /// are built on it.
    pub destination: PathBuf,
    /// The destination with symlinks resolved, for containment checks.
    pub root: PathBuf,
    pub on_file: Box<dyn Fn(ReceivedFile) + Send + Sync>,
}

/// A running SFTP server. Dropping it stops it too.
pub struct Receiver {
    addr: SocketAddr,
    fingerprint: String,
    task: JoinHandle<()>,
}

impl Receiver {
    /// Prepares the destination (removing partial files a crashed session
    /// left), binds `cfg.bind` and serves until [`Receiver::stop`].
    pub async fn start(
        cfg: ReceiverConfig,
        on_file: impl Fn(ReceivedFile) + Send + Sync + 'static,
    ) -> Result<Receiver, Error> {
        std::fs::create_dir_all(&cfg.destination)?;
        let destination = std::path::absolute(&cfg.destination)?;
        let root = std::fs::canonicalize(&destination)?;
        handler::clean_stale_parts(&root);
        let fingerprint = cfg
            .host_key
            .public_key()
            .fingerprint(HashAlg::Sha256)
            .to_string();
        let config = Arc::new(server::Config {
            methods: MethodSet::from(&[MethodKind::Password, MethodKind::KeyboardInteractive][..]),
            auth_rejection_time: Duration::from_millis(250),
            auth_rejection_time_initial: Some(Duration::ZERO),
            keys: vec![cfg.host_key],
            nodelay: true,
            ..Default::default()
        });
        let listener = TcpListener::bind(cfg.bind).await?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared {
            user: cfg.user,
            password: cfg.password,
            destination,
            root,
            on_file: Box::new(on_file),
        });
        tracing::info!(%addr, destination = %shared.destination.display(), "SFTP receiver listening");
        let task = tokio::spawn(handler::serve(listener, config, shared));
        Ok(Receiver {
            addr,
            fingerprint,
            task,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// `SHA256:<base64 without padding>`, as `ssh-keygen -l` prints it.
    pub fn fingerprint_sha256(&self) -> String {
        self.fingerprint.clone()
    }

    /// Stops listening and closes every live session; unfinished uploads
    /// are discarded.
    pub async fn stop(mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
        tracing::info!("SFTP receiver stopped");
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Loads the ed25519 host key at `path` (OpenSSH PEM), or makes one and
/// saves it there (owner-only on unix), so the fingerprint the camera
/// confirmed stays the same across runs.
pub fn load_or_create_host_key(path: &Path) -> std::io::Result<PrivateKey> {
    use std::io::{Error as IoError, ErrorKind};
    match std::fs::read(path) {
        Ok(pem) => {
            return PrivateKey::from_openssh(&pem)
                .map_err(|e| IoError::new(ErrorKind::InvalidData, e.to_string()))
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
        .map_err(|e| IoError::other(e.to_string()))?;
    let pem = key
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .map_err(|e| IoError::other(e.to_string()))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    std::io::Write::write_all(&mut file, pem.as_bytes())?;
    file.sync_all()?;
    Ok(key)
}
