//! Cameras with Access Authentication: PTP/IP through an SSH tunnel.

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use sony_ptpip::capture::{Destination, DestinationMode};
use sony_ptpip::error::Error;
use sony_ptpip::{normalize_fingerprint, Camera, CameraConfig, CameraEvent, SshConfig};
use sony_ptpip_sim::{spawn, SimCommand, SimConfig, SimHandle, SshSimConfig};
use tokio::sync::broadcast;
use tokio::time::timeout;

/// Longest wait for any one expected event.
const WAIT: Duration = Duration::from_secs(10);

const USER: &str = "cam";
const PASSWORD: &str = "12345678";

async fn sim() -> SimHandle {
    spawn(SimConfig {
        ssh: Some(SshSimConfig {
            user: USER.into(),
            password: PASSWORD.into(),
        }),
        ..SimConfig::default()
    })
    .await
}

fn ssh_config(sim: &SimHandle, password: &str, fingerprint: Option<String>) -> SshConfig {
    SshConfig {
        user: USER.into(),
        password: password.into(),
        expected_fingerprint: fingerprint,
        port: sim.ssh_addr.unwrap().port(),
    }
}

/// With SSH set, the camera's PTP/IP port is only reached through the
/// tunnel, so the address names the camera's own port 15740 (the only one
/// the simulator's SSH front forwards to) rather than the simulator's.
fn config(sim: &SimHandle, dir: &Path, ssh: SshConfig) -> CameraConfig {
    CameraConfig {
        addr: SocketAddr::new(sim.addr.ip(), 15740),
        guid: [0x42; 16],
        host_name: "Rimix on test".into(),
        destination: Destination {
            base: dir.to_path_buf(),
            mode: DestinationMode::CustomFolder,
            recovery_dir: None,
        },
        auto_receive: true,
        reconnect: true,
        before_reconnect: None,
        ssh: Some(ssh),
    }
}

async fn wait_for<T>(
    rx: &mut broadcast::Receiver<CameraEvent>,
    what: &str,
    mut pick: impl FnMut(&CameraEvent) -> Option<T>,
) -> T {
    timeout(WAIT, async {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if let Some(t) = pick(&ev) {
                        return t;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(e) => panic!("event channel failed: {e}"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

fn auth_lines(sim: &SimHandle) -> Vec<String> {
    sim.log
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.starts_with("ssh auth"))
        .cloned()
        .collect()
}

#[tokio::test]
async fn tunnel_connects_a_camera_behind_ssh() {
    let tmp = tempfile::tempdir().unwrap();
    let sim = sim().await;
    let fp = sim.fingerprint_sha256.clone();
    let camera = Camera::connect(config(&sim, tmp.path(), ssh_config(&sim, PASSWORD, fp)))
        .await
        .unwrap();
    let state = camera.state();
    assert!(state.connected && state.secured, "{state:?}");
    assert_eq!(state.model, "ZV-E10");
    let mut rx = camera.subscribe();
    camera.shoot().await.unwrap();
    let saved = wait_for(&mut rx, "CaptureSaved", |ev| match ev {
        CameraEvent::CaptureSaved(r) => Some(r.filename.clone()),
        _ => None,
    })
    .await;
    assert_eq!(saved, "DSC00001.JPG");
    camera.disconnect().await;
    assert!(!camera.state().secured);
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn md5_fingerprint_is_accepted_too() {
    let tmp = tempfile::tempdir().unwrap();
    let sim = sim().await;
    let fp = sim.fingerprint_md5.clone().map(|f| f.to_uppercase());
    let camera = Camera::connect(config(&sim, tmp.path(), ssh_config(&sim, PASSWORD, fp)))
        .await
        .unwrap();
    assert!(camera.state().secured);
    camera.disconnect().await;
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn unknown_fingerprint_is_reported_before_auth() {
    let tmp = tempfile::tempdir().unwrap();
    let sim = sim().await;
    let err = Camera::connect(config(&sim, tmp.path(), ssh_config(&sim, PASSWORD, None)))
        .await
        .unwrap_err();
    match err {
        Error::HostKeyUnknown {
            ref sha256,
            ref md5,
        } => {
            assert_eq!(Some(sha256), sim.fingerprint_sha256.as_ref());
            assert_eq!(Some(md5), sim.fingerprint_md5.as_ref());
        }
        other => panic!("expected HostKeyUnknown, got {other:?}"),
    }
    assert_eq!(
        err.to_string(),
        format!(
            "the camera's fingerprint needs confirming: {}",
            sim.fingerprint_sha256.clone().unwrap()
        )
    );
    assert!(auth_lines(&sim).is_empty(), "{:?}", auth_lines(&sim));
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn wrong_fingerprint_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let sim = sim().await;
    let cfg = ssh_config(&sim, PASSWORD, Some("SHA256:AAAA".into()));
    let err = Camera::connect(config(&sim, tmp.path(), cfg))
        .await
        .unwrap_err();
    match err {
        Error::HostKeyMismatch {
            ref expected,
            ref actual_sha256,
        } => {
            assert_eq!(expected, "SHA256:AAAA");
            assert_eq!(Some(actual_sha256), sim.fingerprint_sha256.as_ref());
        }
        other => panic!("expected HostKeyMismatch, got {other:?}"),
    }
    assert!(err
        .to_string()
        .starts_with("the camera's fingerprint changed"));
    assert!(auth_lines(&sim).is_empty(), "{:?}", auth_lines(&sim));
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn wrong_password_is_ssh_auth() {
    let tmp = tempfile::tempdir().unwrap();
    let sim = sim().await;
    let fp = sim.fingerprint_sha256.clone();
    let err = Camera::connect(config(&sim, tmp.path(), ssh_config(&sim, "wrong", fp)))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::SshAuth), "{err:?}");
    assert_eq!(
        err.to_string(),
        "the camera refused the username or password"
    );
    // A refused keyboard-interactive answer is not tried again as a
    // password: each try counts towards the camera's lockout.
    let refused: Vec<_> = auth_lines(&sim)
        .into_iter()
        .filter(|l| l.ends_with("refused"))
        .collect();
    assert_eq!(
        refused,
        vec!["ssh auth keyboard-interactive cam refused".to_string()]
    );
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn tunnel_is_reopened_on_reconnect() {
    let tmp = tempfile::tempdir().unwrap();
    let sim = sim().await;
    let fp = sim.fingerprint_sha256.clone();
    let camera = Camera::connect(config(&sim, tmp.path(), ssh_config(&sim, PASSWORD, fp)))
        .await
        .unwrap();
    let mut rx = camera.subscribe();
    let logins_before = auth_lines(&sim).len();
    sim.events.send(SimCommand::DropConnection).await.unwrap();
    wait_for(&mut rx, "Disconnected", |ev| {
        matches!(ev, CameraEvent::Disconnected { .. }).then_some(())
    })
    .await;
    wait_for(&mut rx, "Connected", |ev| {
        matches!(ev, CameraEvent::Connected).then_some(())
    })
    .await;
    let state = camera.state();
    assert!(state.connected && state.secured, "{state:?}");
    assert!(
        auth_lines(&sim).len() > logins_before,
        "the tunnel was not reopened: {:?}",
        auth_lines(&sim)
    );
    camera.shoot().await.unwrap();
    camera.disconnect().await;
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn refused_login_on_reconnect_stops_reconnecting() {
    let tmp = tempfile::tempdir().unwrap();
    let sim = sim().await;
    let fp = sim.fingerprint_sha256.clone();
    let camera = Camera::connect(config(&sim, tmp.path(), ssh_config(&sim, PASSWORD, fp)))
        .await
        .unwrap();
    let mut rx = camera.subscribe();
    sim.events
        .send(SimCommand::SetSshPassword("changed-on-camera".into()))
        .await
        .unwrap();
    sim.events.send(SimCommand::DropConnection).await.unwrap();
    let reason = wait_for(
        &mut rx,
        "Disconnected for the refused login",
        |ev| match ev {
            CameraEvent::Disconnected { reason }
                if reason.contains("refused the username or password") =>
            {
                Some(reason.clone())
            }
            _ => None,
        },
    )
    .await;
    assert_eq!(reason, "the camera refused the username or password");
    assert!(!camera.state().connected);
    let more = timeout(Duration::from_secs(3), async {
        loop {
            match rx.recv().await {
                Ok(CameraEvent::Reconnecting { attempt }) => return Some(attempt),
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
    .await;
    assert!(
        !matches!(more, Ok(Some(_))),
        "kept reconnecting after a refused login: {more:?}"
    );
    camera.disconnect().await;
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[test]
fn fingerprints_normalise() {
    assert_eq!(
        normalize_fingerprint("SHA256:AbC/+="),
        normalize_fingerprint("abc/+")
    );
    assert_eq!(normalize_fingerprint("MD5:aa:BB:cc"), "aabbcc");
}

#[test]
fn ssh_config_hides_the_password() {
    let cfg = SshConfig {
        user: "cam".into(),
        password: "s3cret-pass".into(),
        expected_fingerprint: None,
        port: 22,
    };
    let debug = format!("{cfg:?}");
    assert!(!debug.contains("s3cret-pass"), "{debug}");
    assert!(debug.contains("<hidden>"), "{debug}");
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(!json.contains("s3cret-pass"), "{json}");
}
