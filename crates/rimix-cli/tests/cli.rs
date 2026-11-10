use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use sony_ptpip_sim::{spawn, SimCommand, SimConfig, SimHandle, SshSimConfig};

fn rimix(guid: &Path) -> Command {
    let mut cmd = Command::cargo_bin("rimix").unwrap();
    cmd.arg("--guid-file").arg(guid);
    cmd.timeout(std::time::Duration::from_secs(30));
    cmd
}

struct Env {
    sim: SimHandle,
    tmp: tempfile::TempDir,
}

impl Env {
    async fn new(config: SimConfig) -> Env {
        Env {
            sim: spawn(config).await,
            tmp: tempfile::tempdir().unwrap(),
        }
    }
    fn guid(&self) -> std::path::PathBuf {
        self.tmp.path().join("guid")
    }
    fn addr(&self) -> String {
        self.sim.addr.to_string()
    }
    fn out(&self) -> std::path::PathBuf {
        let out = self.tmp.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        out
    }
    async fn finish(self) {
        let _ = self.sim.events.send(SimCommand::Shutdown).await;
    }
}

/// Runs the CLI off the async runtime so the in-process simulator keeps serving.
async fn run(mut cmd: Command) -> std::process::Output {
    tokio::task::spawn_blocking(move || cmd.output().unwrap())
        .await
        .unwrap()
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn discover_lists_simulator() {
    let env = Env::new(SimConfig {
        ssdp_port: Some(0),
        ..SimConfig::default()
    })
    .await;
    let ssdp = env.sim.ssdp_addr.unwrap().to_string();
    let mut cmd = rimix(&env.guid());
    cmd.args(["discover", "--timeout", "1", "--ssdp-target", &ssdp]);
    let out = run(cmd).await;
    assert!(out.status.success());
    assert!(stdout(&out).contains("ZV-E10"), "{}", stdout(&out));
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn connect_prints_three_lines() {
    let env = Env::new(SimConfig::default()).await;
    let mut cmd = rimix(&env.guid());
    cmd.args(["connect", &env.addr()]);
    let out = run(cmd).await;
    assert!(out.status.success());
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert_eq!(lines[0], "Model: ZV-E10");
    assert!(lines[1].starts_with("Firmware: "));
    assert!(lines[2].starts_with("Battery: "));
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn props_shows_shutter() {
    let env = Env::new(SimConfig::default()).await;
    let mut cmd = rimix(&env.guid());
    cmd.args(["props", &env.addr()]);
    let out = run(cmd).await;
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("1/125"), "{text}");
    assert!(text.contains("0xD20D"), "{text}");
    assert!(text.lines().next().unwrap().contains("Writable"));
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn set_iso_then_props_shows_new_value() {
    let env = Env::new(SimConfig::default()).await;
    let mut cmd = rimix(&env.guid());
    cmd.args(["set", &env.addr(), "D21E", "1600"]);
    let out = run(cmd).await;
    assert!(out.status.success(), "{:?}", out);
    assert_eq!(stdout(&out).trim(), "Set 0xD21E to ISO 1600");
    let mut cmd = rimix(&env.guid());
    cmd.args(["props", &env.addr()]);
    let out = run(cmd).await;
    assert!(stdout(&out).contains("1600"), "{}", stdout(&out));
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn set_rejects_a_value_the_camera_refuses() {
    let env = Env::new(SimConfig::default()).await;
    let mut cmd = rimix(&env.guid());
    cmd.args(["set", &env.addr(), "0xD21E", "12345"]);
    let out = run(cmd).await;
    assert_eq!(out.status.code(), Some(4));
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn shoot_saves_a_file() {
    let env = Env::new(SimConfig::default()).await;
    let out_dir = env.out();
    let mut cmd = rimix(&env.guid());
    cmd.args(["shoot", &env.addr(), "--out"]).arg(&out_dir);
    let out = run(cmd).await;
    assert!(out.status.success(), "{:?}", out);
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "{text}");
    assert!(lines[0].starts_with("Saved "), "{text}");
    let files: Vec<_> = std::fs::read_dir(&out_dir).unwrap().collect();
    assert_eq!(files.len(), 1);
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn shoot_saves_every_file_of_the_shot() {
    // One release gives two files, as RAW+JPEG does.
    let env = Env::new(SimConfig {
        files_per_shot: 2,
        ..SimConfig::default()
    })
    .await;
    let out_dir = env.out();
    let mut cmd = rimix(&env.guid());
    cmd.args(["shoot", &env.addr(), "--out"]).arg(&out_dir);
    let out = run(cmd).await;
    assert!(out.status.success(), "{:?}", out);
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(lines.iter().all(|l| l.starts_with("Saved ")), "{text}");
    let mut files: Vec<String> = std::fs::read_dir(&out_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert_eq!(files, ["DSC00001.JPG", "DSC00002.JPG"]);
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn liveview_saves_three_frames() {
    let env = Env::new(SimConfig::default()).await;
    let out_dir = env.out();
    let mut cmd = rimix(&env.guid());
    cmd.args(["liveview", &env.addr(), "--frames", "3", "--out"])
        .arg(&out_dir);
    let out = run(cmd).await;
    assert!(out.status.success(), "{:?}", out);
    for n in 1..=3 {
        let bytes = std::fs::read(out_dir.join(format!("frame-{n:04}.jpg"))).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
    }
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn record_reports_the_new_state() {
    let env = Env::new(SimConfig::default()).await;
    let mut cmd = rimix(&env.guid());
    cmd.args(["record", &env.addr()]);
    let out = run(cmd).await;
    assert!(out.status.success(), "{:?}", out);
    assert_eq!(stdout(&out).trim(), "Recording started");
    env.finish().await;
}

/// A simulator behind an SSH front, like a camera with Access
/// Authentication on.
async fn ssh_env() -> Env {
    Env::new(SimConfig {
        ssh: Some(SshSimConfig {
            user: "cam".into(),
            password: "12345678".into(),
        }),
        ..SimConfig::default()
    })
    .await
}

/// Through SSH the camera is named by its IP: its own port 15740 is only
/// reached through the tunnel, here on the simulator's SSH port.
fn ssh_args(env: &Env, cmd: &mut Command) {
    let ssh_port = env.sim.ssh_addr.unwrap().port().to_string();
    cmd.arg(env.sim.addr.ip().to_string()).args([
        "--ssh-user",
        "cam",
        "--ssh-password-stdin",
        "--ssh-port",
        &ssh_port,
    ]);
    cmd.write_stdin("12345678\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn shoot_through_ssh_saves_a_file() {
    let env = ssh_env().await;
    let out_dir = env.out();
    let fp = env.sim.fingerprint_sha256.clone().unwrap();
    let mut cmd = rimix(&env.guid());
    cmd.arg("shoot");
    ssh_args(&env, &mut cmd);
    cmd.args(["--fingerprint", &fp, "--out"]).arg(&out_dir);
    let out = run(cmd).await;
    assert!(out.status.success(), "{:?}", out);
    assert!(stdout(&out).starts_with("Saved "), "{}", stdout(&out));
    assert_eq!(std::fs::read_dir(&out_dir).unwrap().count(), 1);
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unconfirmed_fingerprint_is_printed_and_exits_3() {
    let env = ssh_env().await;
    let sha256 = env.sim.fingerprint_sha256.clone().unwrap();
    let md5 = env.sim.fingerprint_md5.clone().unwrap();
    let mut cmd = rimix(&env.guid());
    cmd.arg("connect");
    ssh_args(&env, &mut cmd);
    let out = run(cmd).await;
    assert_eq!(out.status.code(), Some(3), "{:?}", out);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(&format!(
            "Fingerprint: {sha256} (md5 {md5})\nConfirm it on the camera and pass --fingerprint"
        )),
        "{err}"
    );
    assert!(!err.contains("12345678"), "{err}");
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_ssh_password_exits_4() {
    let env = ssh_env().await;
    let fp = env.sim.fingerprint_sha256.clone().unwrap();
    let mut cmd = rimix(&env.guid());
    cmd.arg("props");
    ssh_args(&env, &mut cmd);
    cmd.args(["--fingerprint", &fp]).write_stdin("wrong\n");
    let out = run(cmd).await;
    assert_eq!(out.status.code(), Some(4), "{:?}", out);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("refused the username or password"), "{err}");
    env.finish().await;
}

#[test]
fn ssh_user_needs_the_password_on_stdin() {
    let tmp = tempfile::tempdir().unwrap();
    rimix(&tmp.path().join("guid"))
        .args(["shoot", "127.0.0.1", "--ssh-user", "cam"])
        .assert()
        .code(2);
}

#[test]
fn connect_to_closed_port_exits_3() {
    let tmp = tempfile::tempdir().unwrap();
    // Bind and drop to find a port nothing listens on.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    rimix(&tmp.path().join("guid"))
        .args(["connect", &format!("127.0.0.1:{port}")])
        .assert()
        .code(3)
        .stderr(predicate::str::is_empty().not());
}

#[test]
fn bad_arguments_exit_2() {
    let tmp = tempfile::tempdir().unwrap();
    rimix(&tmp.path().join("guid"))
        .args(["set", "127.0.0.1", "zz", "1"])
        .assert()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn watch_reconnects_after_a_drop() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command as Std, Stdio};
    use std::sync::mpsc;

    let env = Env::new(SimConfig::default()).await;
    let out_dir = env.out();
    let mut child = Std::new(assert_cmd::cargo::cargo_bin("rimix"))
        .arg("--guid-file")
        .arg(env.guid())
        .args(["watch", &env.addr(), "--exit-after", "20", "--out"])
        .arg(&out_dir)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let (tx, rx) = mpsc::channel();
    let stdout = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    fn wait(rx: &mpsc::Receiver<String>, want: &str) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match rx.recv_timeout(left) {
                Ok(line) if line.contains(want) => return,
                Ok(_) => {}
                Err(_) => panic!("no line containing {want:?} within 10 s"),
            }
        }
    }
    wait(&rx, "connected");
    env.sim
        .events
        .send(SimCommand::DropConnection)
        .await
        .unwrap();
    let waited = tokio::task::spawn_blocking(move || {
        wait(&rx, "disconnected");
        wait(&rx, "reconnecting #");
        wait(&rx, "connected");
    })
    .await;
    let _ = child.kill();
    let _ = child.wait();
    waited.unwrap();
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn watch_exits_4_when_the_camera_refuses_the_login_on_reconnect() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command as Std, Stdio};

    let env = ssh_env().await;
    let out_dir = env.out();
    let fp = env.sim.fingerprint_sha256.clone().unwrap();
    let ssh_port = env.sim.ssh_addr.unwrap().port().to_string();
    let mut child = Std::new(assert_cmd::cargo::cargo_bin("rimix"))
        .arg("--guid-file")
        .arg(env.guid())
        .args(["watch", &env.sim.addr.ip().to_string()])
        .args(["--ssh-user", "cam", "--ssh-password-stdin", "--ssh-port"])
        .arg(&ssh_port)
        .args(["--fingerprint", &fp, "--exit-after", "25", "--out"])
        .arg(&out_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"12345678\n")
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let first = tokio::task::spawn_blocking(move || {
        let line = lines.next().unwrap().unwrap();
        (line, lines)
    })
    .await
    .unwrap();
    assert_eq!(first.0, "connected");
    let mut lines = first.1;
    env.sim
        .events
        .send(SimCommand::SetSshPassword("changed".into()))
        .await
        .unwrap();
    env.sim
        .events
        .send(SimCommand::DropConnection)
        .await
        .unwrap();
    let (status, rest) = tokio::task::spawn_blocking(move || {
        let rest: Vec<String> = lines.by_ref().map_while(Result::ok).collect();
        (child.wait().unwrap(), rest)
    })
    .await
    .unwrap();
    assert_eq!(status.code(), Some(4), "{rest:?}");
    assert!(
        rest.iter()
            .any(|l| l.contains("refused the username or password")),
        "{rest:?}"
    );
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn step_moves_shutter_to_the_neighbouring_value() {
    let env = Env::new(SimConfig::default()).await;
    let mut cmd = rimix(&env.guid());
    cmd.args(["step", &env.addr(), "D20D", "+1"]);
    let out = run(cmd).await;
    assert!(out.status.success(), "{:?}", out);
    let text = stdout(&out);
    assert!(text.starts_with("Set 0xD20D to "), "{text}");
    assert!(!text.contains("1/125"), "{text}");
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn props_all_lists_more_rows_than_the_default() {
    let env = Env::new(SimConfig::default()).await;
    let mut default = rimix(&env.guid());
    default.args(["props", &env.addr()]);
    let mut all = rimix(&env.guid());
    all.args(["props", &env.addr(), "--all"]);
    let default = stdout(&run(default).await).lines().count();
    let all = stdout(&run(all).await).lines().count();
    assert!(all > default, "{all} rows vs {default}");
    env.finish().await;
}

#[test]
fn wifi_status_prints_the_device() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = rimix(&tmp.path().join("guid"));
    cmd.env("RIMIX_WIFI", "fake").args(["wifi", "status"]);
    cmd.assert()
        .success()
        .stdout("device: fake0\nnetwork: none\naddress: none\n");
}

#[test]
fn wifi_join_in_fake_mode_prints_the_report() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = rimix(&tmp.path().join("guid"));
    cmd.env("RIMIX_WIFI", "fake")
        .args(["wifi", "join", "DIRECT-abc1:ZV-E10", "--password-stdin"])
        .write_stdin("12345678\n");
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    assert!(text.contains("attempts: 1"), "{text}");
    assert!(!text.contains("12345678"), "{text}");
}

const MIB: u64 = 1 << 20;

/// A hash of `bytes`, to compare a downloaded file with what it should hold.
fn hash_of(bytes: impl IntoIterator<Item = u8>) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for b in bytes {
        h.write_u8(b);
    }
    h.finish()
}

/// The hash of the simulator's card file `handle` of `len` bytes: byte `i`
/// is `(i * 31 + handle) as u8`.
fn sim_hash(handle: u32, len: u64) -> u64 {
    hash_of((0..len).map(|i| i.wrapping_mul(31).wrapping_add(u64::from(handle)) as u8))
}

fn file_hash(path: &Path) -> u64 {
    hash_of(std::fs::read(path).unwrap())
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn card_list_prints_the_files_newest_first() {
    let env = Env::new(SimConfig::default()).await;
    let mut cmd = rimix(&env.guid());
    cmd.args(["card", "list", &env.addr()]);
    let out = run(cmd).await;
    assert!(out.status.success(), "{out:?}");
    let text = stdout(&out);
    let rows: Vec<Vec<&str>> = text
        .lines()
        .map(|l| {
            l.split("  ")
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .collect()
        })
        .collect();
    assert_eq!(
        rows,
        [
            vec!["Kind", "Size", "Date", "Handle", "Name"],
            vec!["Video", "64 MB", "2026-11-10 10:20", "259", "C0001.MP4"],
            vec!["Raw", "24 MB", "2026-11-10 10:15", "258", "DSC00010.ARW"],
            vec!["Still", "2.0 MB", "2026-11-10 10:15", "257", "DSC00010.JPG"],
        ],
        "{text}"
    );
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn card_get_by_name_saves_the_simulators_bytes() {
    let env = Env::new(SimConfig::default()).await;
    let out_dir = env.out();
    let mut cmd = rimix(&env.guid());
    cmd.args(["card", "get", &env.addr(), "dsc00010.jpg", "--out"])
        .arg(&out_dir);
    let out = run(cmd).await;
    assert!(out.status.success(), "{out:?}");
    let saved = out_dir.join("DSC00010.JPG");
    assert_eq!(stdout(&out).trim(), saved.display().to_string());
    assert_eq!(file_hash(&saved), sim_hash(257, 2 * MIB));
    let progress = stderr(&out);
    assert!(progress.contains("1.0 MB of 2.0 MB"), "{progress}");
    assert!(progress.contains("2.0 MB of 2.0 MB (100%)"), "{progress}");
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn card_get_by_handle_saves_the_file() {
    let env = Env::new(SimConfig::default()).await;
    let out_dir = env.out();
    let mut cmd = rimix(&env.guid());
    cmd.args(["card", "get", &env.addr(), "258", "--out"])
        .arg(&out_dir);
    let out = run(cmd).await;
    assert!(out.status.success(), "{out:?}");
    let saved = out_dir.join("DSC00010.ARW");
    assert_eq!(file_hash(&saved), sim_hash(258, 24 * MIB));
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn card_get_resumes_after_a_drop() {
    let env = Env::new(SimConfig {
        drop_during_card_download_at: Some(1_500_000),
        ..SimConfig::default()
    })
    .await;
    let out_dir = env.out();
    let get = || {
        let mut cmd = rimix(&env.guid());
        cmd.args(["card", "get", &env.addr(), "DSC00010.JPG", "--out"])
            .arg(&out_dir);
        cmd
    };
    let out = run(get()).await;
    assert_eq!(out.status.code(), Some(3), "{out:?}");
    assert!(!out_dir.join("DSC00010.JPG").exists());
    let from = env.sim.log.lock().unwrap().len();

    let out = run(get()).await;
    assert!(out.status.success(), "{out:?}");
    let saved = out_dir.join("DSC00010.JPG");
    assert_eq!(file_hash(&saved), sim_hash(257, 2 * MIB));
    // The second run asked only for what the first did not keep.
    let log = env.sim.log.lock().unwrap()[from..].join("\n");
    assert!(log.contains("offset=1048576"), "{log}");
    assert!(!log.contains("offset=0 "), "{log}");
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn card_get_of_an_unknown_name_exits_2() {
    let env = Env::new(SimConfig::default()).await;
    let mut cmd = rimix(&env.guid());
    cmd.args(["card", "get", &env.addr(), "NOPE.JPG"]);
    let out = run(cmd).await;
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(stderr(&out).contains("NOPE.JPG"), "{}", stderr(&out));
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn card_get_of_a_4_gib_file_exits_4() {
    let mut card = SimConfig::default().card;
    card[2].bytes = 5 * 1024 * MIB;
    let env = Env::new(SimConfig {
        card,
        ..SimConfig::default()
    })
    .await;
    let mut cmd = rimix(&env.guid());
    cmd.args(["card", "list", &env.addr()]);
    let out = run(cmd).await;
    assert!(stdout(&out).contains("4 GB+"), "{}", stdout(&out));
    let out_dir = env.out();
    let mut cmd = rimix(&env.guid());
    cmd.args(["card", "get", &env.addr(), "C0001.MP4", "--out"])
        .arg(&out_dir);
    let out = run(cmd).await;
    assert_eq!(out.status.code(), Some(4), "{out:?}");
    assert!(stderr(&out).contains("4 GiB"), "{}", stderr(&out));
    env.finish().await;
}

#[test]
fn card_get_needs_a_file() {
    let tmp = tempfile::tempdir().unwrap();
    rimix(&tmp.path().join("guid"))
        .args(["card", "get", "127.0.0.1"])
        .assert()
        .code(2);
}
