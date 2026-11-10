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
    assert!(stdout(&out).starts_with("Saved "), "{}", stdout(&out));
    let files: Vec<_> = std::fs::read_dir(&out_dir).unwrap().collect();
    assert_eq!(files.len(), 1);
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
