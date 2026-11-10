//! An SSH front for the simulator, like the one newer Sony bodies put in
//! front of PTP/IP for Access Authentication: the client logs in with the
//! account shown on the camera's screen and reaches port 15740 through a
//! `direct-tcpip` forward.

use std::borrow::Cow;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::keys::{Algorithm, HashAlg, PrivateKey};
use russh::server::{self, Auth, ChannelOpenHandle, Msg, Response, Session};
use russh::{Channel, ChannelOpenFailure, MethodKind, MethodSet};
use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

/// The account the simulated camera shows on its screen.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SshSimConfig {
    pub user: String,
    pub password: String,
}

/// The only port a forward may reach, as on the camera.
const PTP_PORT: u32 = 15740;

/// Bad logins on one connection before the camera hangs up.
const MAX_FAILURES: u32 = 3;

const PASSWORD_PROMPT: &[(Cow<'static, str>, bool)] = &[(Cow::Borrowed("Password:"), false)];

/// A running SSH front.
pub(crate) struct SshFront {
    pub addr: SocketAddr,
    pub fingerprint_sha256: String,
    pub fingerprint_md5: String,
}

/// Binds the SSH server on `bind_ip` with an OS-chosen port and a fresh
/// ed25519 host key, spawning it into `tasks`. Forwards go to `ptp`. Each
/// login attempt is logged to `log` as `ssh auth <method> <user> accepted`
/// or `... refused` (never the password).
pub(crate) async fn start(
    account: Arc<Mutex<SshSimConfig>>,
    log: Arc<Mutex<Vec<String>>>,
    bind_ip: IpAddr,
    ptp: SocketAddr,
    tasks: &mut JoinSet<()>,
) -> SshFront {
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
        .expect("generate simulator SSH host key");
    let public = key.public_key();
    let fingerprint_sha256 = public.fingerprint(HashAlg::Sha256).to_string();
    let fingerprint_md5 = md5_fingerprint(&public.to_bytes().expect("encode SSH host key"));
    let config = Arc::new(server::Config {
        methods: MethodSet::from(&[MethodKind::Password, MethodKind::KeyboardInteractive][..]),
        auth_rejection_time: Duration::from_millis(50),
        auth_rejection_time_initial: Some(Duration::ZERO),
        keys: vec![key],
        nodelay: true,
        ..Default::default()
    });
    let listener = TcpListener::bind(SocketAddr::new(bind_ip, 0))
        .await
        .expect("bind simulator SSH server");
    let addr = listener.local_addr().expect("SSH server address");
    // A PTP listener on 0.0.0.0 or [::] is reached over loopback.
    let ptp = match ptp.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => {
            SocketAddr::new(Ipv4Addr::LOCALHOST.into(), ptp.port())
        }
        IpAddr::V6(ip) if ip.is_unspecified() => {
            SocketAddr::new(Ipv6Addr::LOCALHOST.into(), ptp.port())
        }
        _ => ptp,
    };
    tasks.spawn(serve(listener, config, account, ptp, log));
    SshFront {
        addr,
        fingerprint_sha256,
        fingerprint_md5,
    }
}

/// `aa:bb:...`, as `ssh-keygen -E md5 -l` prints it.
fn md5_fingerprint(public_key_blob: &[u8]) -> String {
    md5::compute(public_key_blob)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Accepts SSH connections until the task is aborted; aborting it drops
/// every connection with it.
async fn serve(
    listener: TcpListener,
    config: Arc<server::Config>,
    account: Arc<Mutex<SshSimConfig>>,
    ptp: SocketAddr,
    log: Arc<Mutex<Vec<String>>>,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((tcp, _)) => {
                    let handler = Camera {
                        account: account.clone(),
                        ptp,
                        log: log.clone(),
                        failures: 0,
                        forwards: JoinSet::new(),
                    };
                    connections.spawn(serve_connection(tcp, config.clone(), handler));
                }
                Err(e) => tracing::warn!(error = %e, "simulator SSH accept failed"),
            },
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

/// Runs one SSH session. russh runs the session in a task of its own, so it
/// is fed through an in-memory pipe that this task pumps: when this task is
/// aborted the pipe closes and the session ends with it.
async fn serve_connection(mut tcp: TcpStream, config: Arc<server::Config>, handler: Camera) {
    let (mut ours, theirs) = tokio::io::duplex(64 * 1024);
    let session = async {
        match server::run_stream(config, theirs, handler).await {
            Ok(running) => running.await,
            Err(e) => Err(e),
        }
    };
    tokio::select! {
        _ = tokio::io::copy_bidirectional(&mut tcp, &mut ours) => {}
        result = session => {
            if let Err(e) = result {
                tracing::debug!(error = %e, "simulator SSH session ended");
            }
        }
    }
}

/// One connection's view of the camera's SSH server.
struct Camera {
    account: Arc<Mutex<SshSimConfig>>,
    ptp: SocketAddr,
    log: Arc<Mutex<Vec<String>>>,
    failures: u32,
    /// Forwards opened on this connection; dropped with the session.
    forwards: JoinSet<()>,
}

impl Camera {
    fn note(&self, method: &str, user: &str, outcome: &str) {
        self.log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(format!("ssh auth {method} {user} {outcome}"));
    }

    fn check(&mut self, method: &str, user: &str, password: &[u8]) -> Result<Auth, russh::Error> {
        let ok = {
            let account = self.account.lock().unwrap_or_else(|e| e.into_inner());
            user == account.user && password == account.password.as_bytes()
        };
        if ok {
            self.note(method, user, "accepted");
            return Ok(Auth::Accept);
        }
        self.note(method, user, "refused");
        self.failures += 1;
        tracing::debug!(
            user,
            failures = self.failures,
            "simulator SSH login refused"
        );
        if self.failures >= MAX_FAILURES {
            // An error from the handler ends the session, as the camera's
            // lockout hangs up.
            return Err(russh::Error::NotAuthenticated);
        }
        Ok(Auth::reject())
    }
}

impl server::Handler for Camera {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        self.check("password", user, password.as_bytes())
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        user: &str,
        _submethods: &str,
        response: Option<Response<'a>>,
    ) -> Result<Auth, Self::Error> {
        match response {
            None => {
                self.note("keyboard-interactive", user, "prompted");
                Ok(Auth::Partial {
                    name: Cow::Borrowed(""),
                    instructions: Cow::Borrowed(""),
                    prompts: Cow::Borrowed(PASSWORD_PROMPT),
                })
            }
            Some(mut answers) => {
                let password = answers.next().unwrap_or_default();
                self.check("keyboard-interactive", user, &password)
            }
        }
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if port_to_connect != PTP_PORT {
            tracing::debug!(
                host_to_connect,
                port_to_connect,
                "simulator SSH forward refused"
            );
            reply
                .reject(ChannelOpenFailure::AdministrativelyProhibited)
                .await;
            return Ok(());
        }
        let mut ptp = match TcpStream::connect(self.ptp).await {
            Ok(stream) => stream,
            Err(e) => {
                tracing::warn!(error = %e, "simulator SSH forward cannot reach PTP/IP");
                reply.reject(ChannelOpenFailure::ConnectFailed).await;
                return Ok(());
            }
        };
        reply.accept().await;
        while self.forwards.try_join_next().is_some() {}
        self.forwards.spawn(async move {
            let mut channel = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut channel, &mut ptp).await;
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use russh::client;
    use russh::keys::PublicKeyOrCertificate;
    use sony_ptpip::ptpip::packet::{read_frame, Packet};
    use tokio::io::AsyncWriteExt;

    use crate::{spawn, SimCommand, SimConfig, SshSimConfig};

    struct AnyHost;

    impl client::Handler for AnyHost {
        type Error = russh::Error;

        async fn check_server_key(
            &mut self,
            _key: &PublicKeyOrCertificate,
        ) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }

    async fn connect(addr: std::net::SocketAddr) -> client::Handle<AnyHost> {
        client::connect(Arc::new(client::Config::default()), addr, AnyHost)
            .await
            .expect("ssh connect")
    }

    #[tokio::test]
    async fn ssh_front_forwards_a_ptp_session() {
        let sim = spawn(SimConfig {
            ssh: Some(SshSimConfig {
                user: "cam".into(),
                password: "12345678".into(),
            }),
            ..SimConfig::default()
        })
        .await;
        let ssh_addr = sim.ssh_addr.expect("ssh address");
        let sha = sim.fingerprint_sha256.clone().expect("sha256 fingerprint");
        assert!(sha.starts_with("SHA256:") && !sha.ends_with('='), "{sha}");
        let md5 = sim.fingerprint_md5.clone().expect("md5 fingerprint");
        assert_eq!(md5.len(), 16 * 3 - 1, "{md5}");
        assert!(md5.split(':').all(|b| b.len() == 2), "{md5}");

        // The right password opens a forward to the PTP/IP port.
        let mut ssh = connect(ssh_addr).await;
        let auth = ssh.authenticate_password("cam", "12345678").await.unwrap();
        assert!(auth.success(), "{auth:?}");
        let channel = ssh
            .channel_open_direct_tcpip("127.0.0.1", 15740, "127.0.0.1", 50000)
            .await
            .expect("direct-tcpip to 15740");
        let mut stream = channel.into_stream();
        let request = Packet::InitCommandRequest {
            guid: [0x11; 16],
            name: "Rimix".into(),
            version: 0x0001_0000,
        };
        stream.write_all(&request.encode()).await.unwrap();
        let ack = Packet::decode(&read_frame(&mut stream).await.unwrap()).unwrap();
        assert!(
            matches!(ack, Packet::InitCommandAck { ref name, .. } if name == "ZV-E10"),
            "{ack:?}"
        );

        // Any other port is refused.
        assert!(ssh
            .channel_open_direct_tcpip("127.0.0.1", 22, "127.0.0.1", 50001)
            .await
            .is_err());

        // Wrong passwords fail, and the third closes the connection.
        let mut bad = connect(ssh_addr).await;
        for _ in 0..2 {
            let auth = bad.authenticate_password("cam", "wrong").await.unwrap();
            assert!(
                matches!(auth, client::AuthResult::Failure { .. }),
                "{auth:?}"
            );
        }
        let third = bad.authenticate_password("cam", "wrong").await;
        assert!(!matches!(third, Ok(ref a) if a.success()), "{third:?}");
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !bad.is_closed() {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("connection closed after three bad passwords");

        // The wrong user is refused even with the right password.
        let mut other = connect(ssh_addr).await;
        let auth = other
            .authenticate_password("root", "12345678")
            .await
            .unwrap();
        assert!(!auth.success());

        let _ = sim.events.send(SimCommand::Shutdown).await;
    }

    #[tokio::test]
    async fn ssh_front_accepts_keyboard_interactive() {
        let sim = spawn(SimConfig {
            ssh: Some(SshSimConfig {
                user: "cam".into(),
                password: "12345678".into(),
            }),
            ..SimConfig::default()
        })
        .await;
        let mut ssh = connect(sim.ssh_addr.unwrap()).await;
        let reply = ssh
            .authenticate_keyboard_interactive_start("cam", None)
            .await
            .unwrap();
        match reply {
            client::KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                assert_eq!(prompts.len(), 1);
                assert_eq!(prompts[0].prompt, "Password:");
                assert!(!prompts[0].echo);
            }
            other => panic!("expected a password prompt, got {other:?}"),
        }
        let reply = ssh
            .authenticate_keyboard_interactive_respond(vec!["12345678".into()])
            .await
            .unwrap();
        assert!(
            matches!(reply, client::KeyboardInteractiveAuthResponse::Success),
            "{reply:?}"
        );
        let _ = sim.events.send(SimCommand::Shutdown).await;
    }

    #[tokio::test]
    async fn shutdown_closes_ssh_sessions() {
        let sim = spawn(SimConfig {
            ssh: Some(SshSimConfig {
                user: "cam".into(),
                password: "12345678".into(),
            }),
            ..SimConfig::default()
        })
        .await;
        let ssh_addr = sim.ssh_addr.unwrap();
        let mut ssh = connect(ssh_addr).await;
        assert!(ssh
            .authenticate_password("cam", "12345678")
            .await
            .unwrap()
            .success());
        sim.events.send(SimCommand::Shutdown).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !ssh.is_closed() {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("session closed on shutdown");
        assert!(tokio::net::TcpStream::connect(ssh_addr).await.is_err());
    }
}
