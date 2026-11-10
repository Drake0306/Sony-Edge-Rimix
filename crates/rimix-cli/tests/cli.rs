use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use sony_ptpip_sim::{spawn, SimCommand, SimConfig, SimHandle};

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
