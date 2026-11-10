//! PTP/IP through SSH, for cameras with Access Authentication (Sony bodies
//! from 2022 on): the camera runs an SSH server, shows its account and host
//! key fingerprint on screen, and port 15740 is only reached through a
//! `direct-tcpip` forward.
//!
//! [`SshTunnel::open`] logs in and listens on a local port; each connection
//! accepted there is forwarded to the camera's PTP/IP port, so the rest of
//! the library dials [`SshTunnel::local_addr`] as if it were the camera.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use russh::client::{self, Handle, KeyboardInteractiveAuthResponse};
use russh::keys::{HashAlg, PublicKey, PublicKeyOrCertificate};
use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::timeout;

use crate::error::Error;

/// How long connecting and logging in may take.
const OPEN_TIMEOUT: Duration = Duration::from_secs(15);
/// How often the session checks the camera is still there while idle.
const KEEPALIVE: Duration = Duration::from_secs(10);
/// Keyboard-interactive prompt rounds answered before trying a password.
const MAX_PROMPT_ROUNDS: usize = 3;
/// The camera-side host each forward asks for, in order: `localhost` as
/// Sony's own tools use, then the loopback address if the camera refuses
/// that spelling.
const FORWARD_HOSTS: [&str; 2] = ["localhost", "127.0.0.1"];

fn default_ssh_port() -> u16 {
    22
}

/// The camera's SSH account, as shown on its screen, and the host key
/// fingerprint the user confirmed.
#[derive(Clone, Serialize, Deserialize)]
pub struct SshConfig {
    pub user: String,
    /// Never stored or logged.
    #[serde(skip)]
    pub password: String,
    /// `SHA256:<base64>` or MD5 `aa:bb:...`; `None` until the user confirms
    /// one, in which case connecting fails with [`Error::HostKeyUnknown`].
    pub expected_fingerprint: Option<String>,
    /// The camera's SSH port: 22 on real cameras.
    #[serde(default = "default_ssh_port")]
    pub port: u16,
}

impl Default for SshConfig {
    fn default() -> Self {
        SshConfig {
            user: String::new(),
            password: String::new(),
            expected_fingerprint: None,
            port: default_ssh_port(),
        }
    }
}

impl std::fmt::Debug for SshConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshConfig")
            .field("user", &self.user)
            .field("password", &"<hidden>")
            .field("expected_fingerprint", &self.expected_fingerprint)
            .field("port", &self.port)
            .finish()
    }
}

/// Reduces a fingerprint to a comparable form: without a `SHA256:` or `MD5:`
/// prefix, colons, whitespace or base64 padding, in lower case.
pub fn normalize_fingerprint(s: &str) -> String {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    let body = if lower.starts_with("sha256:") {
        &s[7..]
    } else if lower.starts_with("md5:") {
        &s[4..]
    } else {
        s
    };
    body.chars()
        .filter(|c| *c != ':' && !c.is_whitespace())
        .collect::<String>()
        .trim_end_matches('=')
        .to_ascii_lowercase()
}

/// `aa:bb:...`, as `ssh-keygen -E md5 -l` prints it.
fn md5_fingerprint(public_key_blob: &[u8]) -> String {
    md5::compute(public_key_blob)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// What the host key check can fail with, besides russh's own errors.
#[derive(Debug)]
enum CheckError {
    Russh(russh::Error),
    Mismatch {
        expected: String,
        actual_sha256: String,
    },
}

impl From<russh::Error> for CheckError {
    fn from(e: russh::Error) -> Self {
        CheckError::Russh(e)
    }
}

/// The client side: checks the camera's host key before anything else.
struct HostKeyCheck {
    expected: Option<String>,
    /// The key's (SHA256, MD5) fingerprints, kept for [`Error::HostKeyUnknown`].
    seen: Arc<StdMutex<Option<(String, String)>>>,
}

impl client::Handler for HostKeyCheck {
    type Error = CheckError;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key: PublicKey = server_public_key.public_key();
        let sha256 = key.fingerprint(HashAlg::Sha256).to_string();
        let md5 = match key.to_bytes() {
            Ok(blob) => md5_fingerprint(&blob),
            Err(e) => {
                tracing::debug!(error = %e, "cannot encode the camera's SSH host key");
                String::new()
            }
        };
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = Some((sha256.clone(), md5.clone()));
        let Some(expected) = &self.expected else {
            // Refusing ends the handshake before any login is sent.
            return Ok(false);
        };
        let want = normalize_fingerprint(expected);
        let matches = !want.is_empty()
            && (want == normalize_fingerprint(&sha256)
                || (!md5.is_empty() && want == normalize_fingerprint(&md5)));
        if matches {
            Ok(true)
        } else {
            Err(CheckError::Mismatch {
                expected: expected.clone(),
                actual_sha256: sha256,
            })
        }
    }
}

/// A logged-in SSH session to the camera with a local listener forwarding
/// to its PTP/IP port. Dropping it closes the listener, every forward and
/// the session.
pub struct SshTunnel {
    local: SocketAddr,
    task: JoinHandle<()>,
}

impl SshTunnel {
    /// Connects to the camera's SSH server at `camera` (port 22 on real
    /// cameras), checks its host key against `cfg.expected_fingerprint`,
    /// logs in, and listens on 127.0.0.1 for connections to forward to
    /// `("localhost", forward_to_port)` on the camera (or `127.0.0.1` when
    /// the camera refuses `localhost`).
    pub async fn open(
        camera: SocketAddr,
        cfg: &SshConfig,
        forward_to_port: u16,
    ) -> Result<SshTunnel, Error> {
        let tcp = timeout(OPEN_TIMEOUT, TcpStream::connect(camera))
            .await
            .map_err(|_| Error::Timeout)??;
        if let Err(e) = tcp.set_nodelay(true) {
            tracing::debug!(error = %e, "set_nodelay failed on the SSH socket");
        }
        // The session runs in russh's own task, fed through an in-memory
        // pipe that the tunnel task pumps: when the tunnel goes, the pipe
        // closes and the session ends with it.
        let (ours, theirs) = tokio::io::duplex(64 * 1024);
        let pump = AbortOnDrop(tokio::spawn(pump(tcp, ours)));

        let seen = Arc::new(StdMutex::new(None));
        let handler = HostKeyCheck {
            expected: cfg.expected_fingerprint.clone(),
            seen: seen.clone(),
        };
        let config = Arc::new(client::Config {
            keepalive_interval: Some(KEEPALIVE),
            nodelay: true,
            ..Default::default()
        });
        let connected = timeout(
            OPEN_TIMEOUT,
            client::connect_stream(config, theirs, handler),
        )
        .await
        .map_err(|_| Error::Timeout)?;
        let mut handle = match connected {
            Ok(handle) => handle,
            Err(CheckError::Mismatch {
                expected,
                actual_sha256,
            }) => {
                return Err(Error::HostKeyMismatch {
                    expected,
                    actual_sha256,
                })
            }
            Err(CheckError::Russh(russh::Error::UnknownKey)) => {
                let seen = seen.lock().unwrap_or_else(|e| e.into_inner()).take();
                return Err(match seen {
                    Some((sha256, md5)) => Error::HostKeyUnknown { sha256, md5 },
                    None => Error::Protocol("the camera's SSH host key was refused".into()),
                });
            }
            Err(CheckError::Russh(russh::Error::IO(e))) => return Err(Error::Io(e)),
            Err(CheckError::Russh(e)) => {
                return Err(Error::Protocol(format!("SSH handshake failed: {e}")))
            }
        };
        timeout(OPEN_TIMEOUT, authenticate(&mut handle, cfg))
            .await
            .map_err(|_| Error::Timeout)??;

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let local = listener.local_addr()?;
        tracing::info!(%camera, %local, "SSH tunnel open");
        let task = tokio::spawn(serve(listener, Arc::new(handle), forward_to_port, pump));
        Ok(SshTunnel { local, task })
    }

    /// Where to dial PTP/IP: connections here reach the camera's port.
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl std::fmt::Debug for SshTunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshTunnel")
            .field("local", &self.local)
            .finish_non_exhaustive()
    }
}

/// Aborts a task when dropped.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn pump(mut tcp: TcpStream, mut ours: tokio::io::DuplexStream) {
    if let Err(e) = tokio::io::copy_bidirectional(&mut tcp, &mut ours).await {
        tracing::debug!(error = %e, "SSH connection ended");
    }
}

/// Logs in: keyboard-interactive first, answering every prompt with the
/// password, then plain password unless the camera already refused the
/// answer. The password is never logged.
async fn authenticate(handle: &mut Handle<HostKeyCheck>, cfg: &SshConfig) -> Result<(), Error> {
    match handle
        .authenticate_keyboard_interactive_start(cfg.user.clone(), None)
        .await
    {
        Ok(mut reply) => {
            let mut answered = false;
            for _ in 0..=MAX_PROMPT_ROUNDS {
                match reply {
                    KeyboardInteractiveAuthResponse::Success => return Ok(()),
                    // The camera refused the answer: trying the same
                    // secret as a password would only count twice
                    // towards its lockout.
                    KeyboardInteractiveAuthResponse::Failure { .. } if answered => {
                        return Err(Error::SshAuth)
                    }
                    // Refused before any prompt: the method is unavailable.
                    KeyboardInteractiveAuthResponse::Failure { .. } => break,
                    KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                        answered = true;
                        let answers = vec![cfg.password.clone(); prompts.len()];
                        reply = match handle
                            .authenticate_keyboard_interactive_respond(answers)
                            .await
                        {
                            Ok(reply) => reply,
                            Err(e) => {
                                tracing::debug!(error = %e, "keyboard-interactive login failed");
                                break;
                            }
                        };
                    }
                }
            }
        }
        Err(e) => tracing::debug!(error = %e, "keyboard-interactive login unavailable"),
    }
    if handle.is_closed() {
        return Err(Error::SshAuth);
    }
    match handle
        .authenticate_password(cfg.user.clone(), cfg.password.clone())
        .await
    {
        Ok(result) if result.success() => Ok(()),
        Ok(_) => Err(Error::SshAuth),
        Err(e) => {
            tracing::debug!(error = %e, "password login failed");
            Err(Error::SshAuth)
        }
    }
}

/// Accepts local connections and forwards each through its own channel,
/// until aborted or the SSH connection ends.
async fn serve(
    listener: TcpListener,
    handle: Arc<Handle<HostKeyCheck>>,
    port: u16,
    mut pump: AbortOnDrop,
) {
    let mut forwards = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((local, peer)) => {
                    forwards.spawn(forward(handle.clone(), local, peer, port));
                }
                Err(e) => tracing::debug!(error = %e, "SSH tunnel accept failed"),
            },
            _ = &mut pump.0 => {
                tracing::info!("SSH connection to the camera closed");
                return;
            }
            Some(_) = forwards.join_next(), if !forwards.is_empty() => {}
        }
    }
}

async fn forward(
    handle: Arc<Handle<HostKeyCheck>>,
    mut local: TcpStream,
    peer: SocketAddr,
    port: u16,
) {
    let _ = local.set_nodelay(true);
    let mut channel = None;
    for host in FORWARD_HOSTS {
        match handle
            .channel_open_direct_tcpip(
                host,
                u32::from(port),
                peer.ip().to_string(),
                u32::from(peer.port()),
            )
            .await
        {
            Ok(opened) => {
                channel = Some(opened);
                break;
            }
            Err(e) => tracing::debug!(error = %e, host, port, "SSH forward refused"),
        }
    }
    let Some(channel) = channel else {
        return;
    };
    let mut remote = channel.into_stream();
    if let Err(e) = tokio::io::copy_bidirectional(&mut local, &mut remote).await {
        tracing::debug!(error = %e, "SSH forward ended");
    }
}
