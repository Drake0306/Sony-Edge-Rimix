//! Joining a Wi-Fi network from the app. On macOS this drives the
//! `networksetup` command through a [`Runner`] so tests never touch the
//! radio. The password is only ever passed to that command, never stored.

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos::{system_address, system_current_network, system_device};

/// How long to wait for a joined network to come up.
pub const JOIN_WAIT: Duration = Duration::from_secs(20);
/// How often to check whether it has.
pub const JOIN_POLL: Duration = Duration::from_secs(1);
/// How many times to ask to join while macOS cannot see the network yet.
pub const FIND_RETRIES: u32 = 3;
/// How long to wait between those tries: the scan cache refreshes within a
/// few seconds.
pub const FIND_RETRY_WAIT: Duration = Duration::from_secs(4);

/// What a join did, for diagnosis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinReport {
    /// How many times the join command ran.
    pub attempts: u32,
    /// The network name read back, when macOS shows it.
    pub seen_ssid: Option<String>,
    /// The IPv4 address that counted as joined, when it was the address.
    pub address: Option<String>,
}

/// A command line for the log, with `[password]` at each secret position.
pub fn redact(args: &[&str], secret_positions: &[usize]) -> String {
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            if secret_positions.contains(&i) {
                "[password]"
            } else {
                a
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// What a finished command printed.
pub struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs a program; a trait so tests can script the answers.
pub trait Runner: Send + Sync {
    fn run(&self, program: &str, args: &[&str]) -> std::io::Result<Output>;
}

/// Runs real programs with `std::process::Command`.
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, program: &str, args: &[&str]) -> std::io::Result<Output> {
        let out = std::process::Command::new(program).args(args).output()?;
        Ok(Output {
            success: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum WifiError {
    NoWifi,
    Unsupported,
    NotVisible(String),
    Failed(String),
}

impl fmt::Display for WifiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WifiError::NoWifi => f.write_str("This computer has no Wi-Fi"),
            WifiError::Unsupported => {
                f.write_str("Joining Wi-Fi networks is not supported on this system yet")
            }
            WifiError::Failed(text) => f.write_str(text),
            WifiError::NotVisible(name) => {
                write!(f, "This computer can't see the network {name} yet")
            }
        }
    }
}

impl std::error::Error for WifiError {}

enum Mode {
    System(Arc<dyn Runner>),
    /// Development: joins at once and remembers the name.
    Fake(Mutex<Option<String>>),
}

/// Cheap to clone: clones share the same state.
#[derive(Clone)]
pub struct Wifi {
    mode: Arc<Mode>,
    sleep: Arc<dyn Fn(Duration) + Send + Sync>,
    now: Arc<dyn Fn() -> Instant + Send + Sync>,
}

impl Wifi {
    pub fn new(runner: Arc<dyn Runner>) -> Wifi {
        Wifi {
            mode: Arc::new(Mode::System(runner)),
            sleep: Arc::new(std::thread::sleep),
            now: Arc::new(Instant::now),
        }
    }

    /// A Wi-Fi that never touches the radio.
    pub fn fake() -> Wifi {
        Wifi {
            mode: Arc::new(Mode::Fake(Mutex::new(None))),
            sleep: Arc::new(std::thread::sleep),
            now: Arc::new(Instant::now),
        }
    }

    /// The fake under `RIMIX_WIFI=fake`, the real thing otherwise.
    pub fn from_env(runner: Arc<dyn Runner>) -> Wifi {
        if std::env::var("RIMIX_WIFI").as_deref() == Ok("fake") {
            Wifi::fake()
        } else {
            Wifi::new(runner)
        }
    }

    /// Replaces how polling waits and reads the time, so tests do not wait.
    pub fn with_clock(
        mut self,
        now: impl Fn() -> Instant + Send + Sync + 'static,
        sleep: impl Fn(Duration) + Send + Sync + 'static,
    ) -> Wifi {
        self.now = Arc::new(now);
        self.sleep = Arc::new(sleep);
        self
    }

    /// The Wi-Fi interface name, such as `en0`.
    pub fn device(&self) -> Result<String, WifiError> {
        match &*self.mode {
            Mode::Fake(_) => Ok("fake0".to_string()),
            Mode::System(runner) => system_device(runner.as_ref()),
        }
    }

    /// The network the Wi-Fi is on, if any.
    pub fn current_network(&self) -> Result<Option<String>, WifiError> {
        match &*self.mode {
            Mode::Fake(name) => Ok(name.lock().unwrap_or_else(|e| e.into_inner()).clone()),
            Mode::System(runner) => system_current_network(runner.as_ref()),
        }
    }

    /// The Wi-Fi interface's IPv4 address, if it has one.
    pub fn address(&self) -> Result<Option<String>, WifiError> {
        match &*self.mode {
            Mode::Fake(_) => Ok(None),
            Mode::System(runner) => system_address(runner.as_ref()),
        }
    }

    /// Joins `ssid` and waits until it is up.
    pub fn join(&self, ssid: &str, password: Option<&str>) -> Result<(), WifiError> {
        self.join_report(ssid, password).map(|_| ())
    }

    /// Joins `ssid`, waits until it is up, and says how it went.
    pub fn join_report(&self, ssid: &str, password: Option<&str>) -> Result<JoinReport, WifiError> {
        match &*self.mode {
            Mode::Fake(name) => {
                *name.lock().unwrap_or_else(|e| e.into_inner()) = Some(ssid.to_string());
                Ok(JoinReport {
                    attempts: 1,
                    seen_ssid: Some(ssid.to_string()),
                    address: None,
                })
            }
            Mode::System(runner) => self.system_join(runner.as_ref(), ssid, password),
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl Wifi {
    fn system_join(
        &self,
        _: &dyn Runner,
        _: &str,
        _: Option<&str>,
    ) -> Result<JoinReport, WifiError> {
        let _ = (&self.sleep, &self.now);
        Err(WifiError::Unsupported)
    }
}

#[cfg(not(target_os = "macos"))]
fn system_device(_: &dyn Runner) -> Result<String, WifiError> {
    Err(WifiError::Unsupported)
}

#[cfg(not(target_os = "macos"))]
fn system_current_network(_: &dyn Runner) -> Result<Option<String>, WifiError> {
    Ok(None)
}

#[cfg(not(target_os = "macos"))]
fn system_address(_: &dyn Runner) -> Result<Option<String>, WifiError> {
    Ok(None)
}

/// The `Device:` of the `Wi-Fi` port in `networksetup -listallhardwareports`.
pub fn parse_wifi_device(listing: &str) -> Option<String> {
    let mut in_wifi = false;
    for line in listing.lines() {
        let line = line.trim();
        if let Some(port) = line.strip_prefix("Hardware Port:") {
            in_wifi = port.trim() == "Wi-Fi";
        } else if in_wifi {
            if let Some(dev) = line.strip_prefix("Device:") {
                return Some(dev.trim().to_string());
            }
        }
    }
    None
}

/// The network name in `networksetup -getairportnetwork` output.
pub fn parse_current_network(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|l| l.strip_prefix("Current Wi-Fi Network:"))
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[cfg(target_os = "macos")]
    const SSID: &str = "DIRECT-abc1:ZV-E10";

    struct FakeRunner {
        answers: Mutex<VecDeque<Output>>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl FakeRunner {
        fn scripted(answers: impl IntoIterator<Item = Output>) -> Arc<FakeRunner> {
            Arc::new(FakeRunner {
                answers: Mutex::new(answers.into_iter().collect()),
                calls: Mutex::new(Vec::new()),
            })
        }
        #[cfg(target_os = "macos")]
        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Runner for FakeRunner {
        fn run(&self, program: &str, args: &[&str]) -> std::io::Result<Output> {
            let mut call = vec![program.to_string()];
            call.extend(args.iter().map(|a| a.to_string()));
            self.calls.lock().unwrap().push(call);
            Ok(self
                .answers
                .lock()
                .unwrap()
                .pop_front()
                .expect("the runner was asked more than the test scripted"))
        }
    }

    #[cfg(target_os = "macos")]
    fn out(stdout: &str) -> Output {
        Output {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    #[cfg(target_os = "macos")]
    fn ports() -> Output {
        out("Hardware Port: Wi-Fi\nDevice: en0\nEthernet Address: 60:3e:5f:22:78:90\n")
    }

    #[cfg(target_os = "macos")]
    fn on(name: &str) -> Output {
        out(&format!("Current Wi-Fi Network: {name}\n"))
    }

    #[cfg(target_os = "macos")]
    fn off() -> Output {
        out("You are not associated with an AirPort network.\n")
    }

    #[test]
    fn finds_the_wifi_device_in_the_hardware_port_listing() {
        let listing = "Hardware Port: Ethernet Adapter\nDevice: en5\nEthernet Address: ..\n\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: 60:3e:5f:22:78:90\n";
        assert_eq!(parse_wifi_device(listing).as_deref(), Some("en0"));
        assert_eq!(
            parse_wifi_device("Hardware Port: Thunderbolt 1\nDevice: en1\n"),
            None
        );
    }

    #[test]
    fn reads_the_current_network() {
        assert_eq!(
            parse_current_network("Current Wi-Fi Network: DIRECT-abc1:ZV-E10\n").as_deref(),
            Some("DIRECT-abc1:ZV-E10")
        );
        assert_eq!(
            parse_current_network("You are not associated with an AirPort network.\n"),
            None
        );
    }

    #[cfg(target_os = "macos")]
    fn no_address() -> Output {
        Output {
            success: false,
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    #[cfg(target_os = "macos")]
    fn address() -> Output {
        out("192.168.122.10\n")
    }

    #[cfg(target_os = "macos")]
    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| p.to_string()).collect()
    }

    #[cfg(target_os = "macos")]
    const LIST: &[&str] = &["networksetup", "-listallhardwareports"];
    #[cfg(target_os = "macos")]
    const GET: &[&str] = &["networksetup", "-getairportnetwork", "en0"];
    #[cfg(target_os = "macos")]
    const ADDR: &[&str] = &["ipconfig", "getifaddr", "en0"];

    /// A Wi-Fi whose clock moves only when it sleeps.
    #[cfg(target_os = "macos")]
    fn wifi_with_fake_clock(runner: Arc<FakeRunner>) -> Wifi {
        let start = Instant::now();
        let elapsed = Arc::new(Mutex::new(Duration::ZERO));
        let read = elapsed.clone();
        Wifi::new(runner).with_clock(
            move || start + *read.lock().unwrap(),
            move |d| *elapsed.lock().unwrap() += d,
        )
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn join_succeeds_when_the_address_appears_though_the_name_stays_unreadable() {
        let runner = FakeRunner::scripted([
            ports(),
            off(),
            out(""),
            off(),
            no_address(),
            off(),
            address(),
        ]);
        let wifi = wifi_with_fake_clock(runner.clone());
        wifi.join(SSID, Some("12345678")).unwrap();
        assert_eq!(
            runner.calls(),
            vec![
                argv(LIST),
                argv(GET),
                argv(&[
                    "networksetup",
                    "-setairportnetwork",
                    "en0",
                    SSID,
                    "12345678"
                ]),
                argv(GET),
                argv(ADDR),
                argv(GET),
                argv(ADDR),
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn join_succeeds_when_the_network_name_reads_back() {
        let runner = FakeRunner::scripted([ports(), on("Home"), out(""), on(SSID)]);
        let wifi = wifi_with_fake_clock(runner.clone());
        wifi.join(SSID, Some("12345678")).unwrap();
        assert_eq!(
            runner.calls(),
            vec![
                argv(LIST),
                argv(GET),
                argv(&[
                    "networksetup",
                    "-setairportnetwork",
                    "en0",
                    SSID,
                    "12345678"
                ]),
                argv(GET),
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn join_without_password_omits_the_argument() {
        for password in [None, Some("")] {
            let runner = FakeRunner::scripted([ports(), off(), out(""), on(SSID)]);
            let wifi = wifi_with_fake_clock(runner.clone());
            wifi.join(SSID, password).unwrap();
            assert_eq!(
                runner.calls(),
                vec![
                    argv(LIST),
                    argv(GET),
                    argv(&["networksetup", "-setairportnetwork", "en0", SSID]),
                    argv(GET),
                ]
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn already_joined_runs_nothing_more() {
        let runner = FakeRunner::scripted([ports(), on(SSID)]);
        let wifi = wifi_with_fake_clock(runner.clone());
        wifi.join(SSID, Some("12345678")).unwrap();
        assert_eq!(runner.calls(), vec![argv(LIST), argv(GET)]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn join_reports_networksetup_failure_text() {
        let runner = FakeRunner::scripted([
            ports(),
            off(),
            out("Failed to join network DIRECT-abc1:ZV-E10.\n"),
        ]);
        let wifi = wifi_with_fake_clock(runner);
        let err = wifi.join(SSID, Some("x")).unwrap_err();
        assert!(matches!(err, WifiError::Failed(ref t) if t.contains("Failed to join")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn join_gives_up_when_no_address_ever_comes() {
        // Polls at 0 s, 1 s, .. 20 s: one name read and one address read each.
        let polls = JOIN_WAIT.as_secs() as usize + 1;
        let mut script = vec![ports(), off(), out("")];
        for _ in 0..polls {
            script.push(off());
            script.push(no_address());
        }
        let runner = FakeRunner::scripted(script);
        let wifi = wifi_with_fake_clock(runner.clone());
        let err = wifi.join(SSID, Some("x")).unwrap_err();
        assert_eq!(err, WifiError::Failed(format!("{SSID} did not come up")));
        let calls = runner.calls();
        assert_eq!(calls.len(), 3 + 2 * polls);
        assert_eq!(
            calls.iter().filter(|c| **c == argv(LIST)).count(),
            1,
            "the device is looked up once per join"
        );
        for pair in calls[3..].chunks(2) {
            assert_eq!(pair, [argv(GET), argv(ADDR)]);
        }
    }

    #[cfg(target_os = "macos")]
    fn not_found() -> Output {
        out(&format!("Could not find network {SSID}.\n"))
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn join_retries_when_the_network_is_not_visible_yet() {
        let runner = FakeRunner::scripted([
            ports(),
            off(),
            not_found(),
            not_found(),
            out(""),
            off(),
            out("192.168.122.5\n"),
        ]);
        let wifi = wifi_with_fake_clock(runner.clone());
        let report = wifi.join_report(SSID, Some("pw")).unwrap();
        assert_eq!(report.attempts, 3);
        assert_eq!(report.address.as_deref(), Some("192.168.122.5"));
        assert_eq!(
            runner
                .calls()
                .iter()
                .filter(|c| c.contains(&"-setairportnetwork".to_string()))
                .count(),
            3
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn join_gives_not_visible_after_the_last_retry() {
        let mut script = vec![ports(), off()];
        script.extend((0..FIND_RETRIES).map(|_| not_found()));
        let runner = FakeRunner::scripted(script);
        let slept = Arc::new(Mutex::new(Vec::new()));
        let record = slept.clone();
        let wifi = Wifi::new(runner.clone())
            .with_clock(Instant::now, move |d| record.lock().unwrap().push(d));
        let err = wifi.join(SSID, Some("pw")).unwrap_err();
        assert_eq!(
            *slept.lock().unwrap(),
            vec![FIND_RETRY_WAIT; FIND_RETRIES as usize - 1]
        );
        assert!(matches!(err, WifiError::NotVisible(ref n) if n == "DIRECT-abc1:ZV-E10"));
        assert_eq!(
            err.to_string(),
            "This computer can't see the network DIRECT-abc1:ZV-E10 yet"
        );
        assert_eq!(runner.calls().len(), 2 + FIND_RETRIES as usize);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_address_on_a_different_named_network_does_not_count_as_joined() {
        // The Mac still reads back its old network: its address is not the camera's.
        let polls = JOIN_WAIT.as_secs() as usize + 1;
        let mut script = vec![ports(), off(), out("")];
        script.extend((0..polls).map(|_| on("Home")));
        let runner = FakeRunner::scripted(script);
        let wifi = wifi_with_fake_clock(runner.clone());
        assert!(matches!(
            wifi.join(SSID, None),
            Err(WifiError::Failed(ref t)) if *t == format!("{SSID} did not come up")
        ));
        assert!(!runner.calls().contains(&argv(ADDR)));
    }

    #[test]
    fn redact_hides_the_password_position() {
        assert_eq!(
            redact(
                &["networksetup", "-setairportnetwork", "en0", "X", "secret"],
                &[4]
            ),
            "networksetup -setairportnetwork en0 X [password]"
        );
        assert_eq!(
            redact(&["ipconfig", "getifaddr", "en0"], &[]),
            "ipconfig getifaddr en0"
        );
    }

    #[test]
    fn fake_mode_reports_one_attempt() {
        let report = Wifi::fake()
            .join_report("DIRECT-abc1:ZV-E10", None)
            .unwrap();
        assert_eq!(report.attempts, 1);
        assert_eq!(report.seen_ssid.as_deref(), Some("DIRECT-abc1:ZV-E10"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn no_wifi_port_is_a_plain_error() {
        let runner = FakeRunner::scripted([out("Hardware Port: Thunderbolt 1\nDevice: en1\n")]);
        let wifi = Wifi::new(runner);
        assert!(matches!(wifi.join("x", None), Err(WifiError::NoWifi)));
        assert_eq!(WifiError::NoWifi.to_string(), "This computer has no Wi-Fi");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn other_systems_cannot_join_but_report_no_network() {
        let wifi = Wifi::new(FakeRunner::scripted([]));
        assert_eq!(wifi.join("x", None), Err(WifiError::Unsupported));
        assert_eq!(wifi.device(), Err(WifiError::Unsupported));
        assert_eq!(wifi.current_network(), Ok(None));
    }

    #[test]
    fn fake_mode_joins_at_once_and_remembers_the_name() {
        let wifi = Wifi::fake();
        assert_eq!(wifi.device().unwrap(), "fake0");
        wifi.join("DIRECT-abc1:ZV-E10", None).unwrap();
        assert_eq!(
            wifi.current_network().unwrap().as_deref(),
            Some("DIRECT-abc1:ZV-E10")
        );
    }
}
