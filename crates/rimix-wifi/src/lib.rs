//! Joining a Wi-Fi network from the app. On macOS this drives the
//! `networksetup` command, on Windows `netsh wlan`, through a [`Runner`] so
//! tests never touch the radio. The password is only ever passed to that
//! command (on Windows inside a temporary profile file that is deleted at
//! once), never stored by Rimix and never logged.

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos::{system_address, system_current_network, system_device};
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod run;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows::{system_address, system_current_network, system_device};

/// How long to wait for a joined network to come up.
pub const JOIN_WAIT: Duration = Duration::from_secs(20);
/// How often to check whether it has.
pub const JOIN_POLL: Duration = Duration::from_secs(1);
/// How many times to ask to join while the system cannot see the network yet.
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

    /// Runs `program` with `args`, then one `key=value` argument per pair
    /// (`netsh` style). By default the pairs are joined as plain
    /// `key=value` strings and handed to [`Runner::run`]; on Windows the
    /// system runner passes them as `key="value"` on the raw command line.
    fn run_kv(
        &self,
        program: &str,
        args: &[&str],
        pairs: &[(&str, &str)],
    ) -> std::io::Result<Output> {
        let joined: Vec<String> = pairs.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let mut all: Vec<&str> = args.to_vec();
        all.extend(joined.iter().map(String::as_str));
        self.run(program, &all)
    }
}

/// One `netsh` argument in its documented `key="value"` form: the value is
/// quoted only when it holds a space or a tab, and any `"` in it becomes
/// `\"`. Windows' usual argument quoting would wrap the whole `key=value`
/// in quotes instead, which `netsh` does not document.
pub fn netsh_kv(key: &str, value: &str) -> String {
    let escaped = value.replace('"', "\\\"");
    if value.contains([' ', '\t']) {
        format!("{key}=\"{escaped}\"")
    } else {
        format!("{key}={escaped}")
    }
}

/// Keeps a console window from flashing up for every `netsh` in the app.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Runs real programs with `std::process::Command`.
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, program: &str, args: &[&str]) -> std::io::Result<Output> {
        let mut command = std::process::Command::new(program);
        command.args(args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        output(command)
    }

    #[cfg(windows)]
    fn run_kv(
        &self,
        program: &str,
        args: &[&str],
        pairs: &[(&str, &str)],
    ) -> std::io::Result<Output> {
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new(program);
        command.args(args).creation_flags(CREATE_NO_WINDOW);
        for (key, value) in pairs {
            command.raw_arg(netsh_kv(key, value));
        }
        output(command)
    }
}

fn output(mut command: std::process::Command) -> std::io::Result<Output> {
    let out = command.output()?;
    Ok(Output {
        success: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
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

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn system_device(_: &dyn Runner) -> Result<String, WifiError> {
    Err(WifiError::Unsupported)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn system_current_network(_: &dyn Runner) -> Result<Option<String>, WifiError> {
    Ok(None)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

/// One wireless interface in `netsh wlan show interfaces` output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    pub name: String,
    /// Such as `connected`, `disconnected` or `associating`.
    pub state: String,
    /// Absent while the interface is not on a network.
    pub ssid: Option<String>,
}

impl Interface {
    pub fn is_connected(&self) -> bool {
        self.state.eq_ignore_ascii_case("connected")
    }
}

/// The interfaces in `netsh wlan show interfaces` output, in order. Each
/// `Name` line starts one; its `State` and `SSID` lines follow.
pub fn parse_interfaces(output: &str) -> Vec<Interface> {
    let mut found: Vec<Interface> = Vec::new();
    for line in output.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Name" => found.push(Interface {
                name: value.to_string(),
                state: String::new(),
                ssid: None,
            }),
            "State" => {
                if let Some(last) = found.last_mut() {
                    last.state = value.to_string();
                }
            }
            "SSID" => {
                if let Some(last) = found.last_mut() {
                    last.ssid = Some(value.to_string()).filter(|s| !s.is_empty());
                }
            }
            _ => {}
        }
    }
    found
}

/// `text` with the five XML special characters escaped.
fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// A Windows WLAN profile for a WPA2-Personal network, for
/// `netsh wlan add profile`. It holds the password in plain text: write it
/// only to a temporary file, never to a log.
pub fn profile_xml(ssid: &str, password: &str) -> String {
    let name = xml_escape(ssid);
    let key = xml_escape(password);
    format!(
        r#"<?xml version="1.0"?>
<WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
  <name>{name}</name>
  <SSIDConfig>
    <SSID>
      <name>{name}</name>
    </SSID>
  </SSIDConfig>
  <connectionType>ESS</connectionType>
  <connectionMode>manual</connectionMode>
  <MSM>
    <security>
      <authEncryption>
        <authentication>WPA2PSK</authentication>
        <encryption>AES</encryption>
        <useOneX>false</useOneX>
      </authEncryption>
      <sharedKey>
        <keyType>passPhrase</keyType>
        <protected>false</protected>
        <keyMaterial>{key}</keyMaterial>
      </sharedKey>
    </security>
  </MSM>
</WLANProfile>
"#
    )
}

/// The IPv4 address of the adapter called `interface` in Windows `ipconfig`
/// output. Self-assigned `169.254.x.x` addresses do not count: they mean the
/// network gave out none.
pub fn parse_ipconfig_address(output: &str, interface: &str) -> Option<String> {
    let header = format!(" adapter {interface}:");
    let mut in_adapter = false;
    for line in output.lines() {
        let trimmed = line.trim_end();
        if !trimmed.is_empty() && !trimmed.starts_with(char::is_whitespace) {
            in_adapter = trimmed.ends_with(&header);
            continue;
        }
        if !in_adapter {
            continue;
        }
        let Some((label, value)) = trimmed.split_once(':') else {
            continue;
        };
        // Older Windows says `IP Address`, current Windows `IPv4 Address`.
        let label = label.trim_start();
        if !label.contains("IPv4 Address") && !label.starts_with("IP Address") {
            continue;
        }
        let digits: String = value
            .trim()
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if let Ok(address) = digits.parse::<std::net::Ipv4Addr>() {
            if !address.is_link_local() {
                return Some(address.to_string());
            }
        }
    }
    None
}

/// Writes `xml` to a new temporary `.xml` file, hands its path to
/// `use_file`, and deletes the file afterwards whatever `use_file` returned.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn with_profile_file<T>(
    xml: &str,
    use_file: impl FnOnce(&std::path::Path) -> Result<T, WifiError>,
) -> Result<T, WifiError> {
    use std::io::Write;
    let failed =
        |e: std::io::Error| WifiError::Failed(format!("Could not write the Wi-Fi profile: {e}"));
    let mut file = tempfile::Builder::new()
        .prefix("rimix-wifi-")
        .suffix(".xml")
        .tempfile()
        .map_err(failed)?;
    file.write_all(xml.as_bytes()).map_err(failed)?;
    file.flush().map_err(failed)?;
    // Close our handle before `netsh` opens the file, so its sharing mode
    // cannot clash with an open writer; the TempPath still deletes on drop.
    let path = file.into_temp_path();
    let result = use_file(&path);
    drop(path);
    result
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[cfg(target_os = "macos")]
    const SSID: &str = "DIRECT-abc1:ZV-E10";

    pub(crate) struct FakeRunner {
        answers: Mutex<VecDeque<Output>>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl FakeRunner {
        pub(crate) fn scripted(answers: impl IntoIterator<Item = Output>) -> Arc<FakeRunner> {
            Arc::new(FakeRunner {
                answers: Mutex::new(answers.into_iter().collect()),
                calls: Mutex::new(Vec::new()),
            })
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        pub(crate) fn calls(&self) -> Vec<Vec<String>> {
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

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn other_systems_cannot_join_but_report_no_network() {
        let wifi = Wifi::new(FakeRunner::scripted([]));
        assert_eq!(wifi.join("x", None), Err(WifiError::Unsupported));
        assert_eq!(wifi.device(), Err(WifiError::Unsupported));
        assert_eq!(wifi.current_network(), Ok(None));
    }

    const NETSH_CONNECTED: &str = "\r
There is 1 interface on the system:\r
\r
    Name                   : Wi-Fi\r
    Description            : Intel(R) Wi-Fi 6 AX201 160MHz\r
    GUID                   : 3a5e0c1d-8f7b-4c6e-9d2a-1b0c4e5f6a7b\r
    Physical address       : 60:3e:5f:22:78:90\r
    State                  : connected\r
    SSID                   : DIRECT-abc1:ZV-E10\r
    BSSID                  : 9a:8b:7c:6d:5e:4f\r
    Network type           : Infrastructure\r
    Radio type             : 802.11n\r
    Authentication         : WPA2-Personal\r
    Profile                : DIRECT-abc1:ZV-E10\r
\r
    Hosted network status  : Not available\r
";

    #[test]
    fn parses_netsh_interfaces() {
        assert_eq!(
            parse_interfaces(NETSH_CONNECTED),
            vec![Interface {
                name: "Wi-Fi".to_string(),
                state: "connected".to_string(),
                ssid: Some("DIRECT-abc1:ZV-E10".to_string()),
            }]
        );
        let two = "There are 2 interfaces on the system:\n\n    Name                   : Wi-Fi 2\n    State                  : disconnected\n\n    Name                   : Wi-Fi\n    State                  : associating\n";
        assert_eq!(
            parse_interfaces(two),
            vec![
                Interface {
                    name: "Wi-Fi 2".to_string(),
                    state: "disconnected".to_string(),
                    ssid: None,
                },
                Interface {
                    name: "Wi-Fi".to_string(),
                    state: "associating".to_string(),
                    ssid: None,
                },
            ]
        );
        assert_eq!(
            parse_interfaces("The Wireless AutoConfig Service (wlansvc) is not running.\n"),
            vec![]
        );
    }

    #[test]
    fn profile_xml_is_a_wpa2_psk_profile_for_the_network() {
        let xml = profile_xml("DIRECT-abc1:ZV-E10", "12345678");
        assert!(xml.starts_with("<?xml"));
        assert!(xml.contains("<name>DIRECT-abc1:ZV-E10</name>\n  <SSIDConfig>"));
        assert!(xml.contains("<SSID>\n      <name>DIRECT-abc1:ZV-E10</name>\n    </SSID>"));
        assert!(xml.contains("<keyMaterial>12345678</keyMaterial>"));
        assert!(xml.contains("<authentication>WPA2PSK</authentication>"));
        assert!(xml.contains("<encryption>AES</encryption>"));
        assert!(xml.contains("<keyType>passPhrase</keyType>"));
        assert!(xml.contains("<connectionMode>manual</connectionMode>"));
    }

    #[test]
    fn profile_xml_escapes_markup_in_the_name_and_password() {
        let xml = profile_xml("A&B <cam>", "p\"w'<&>");
        assert!(xml.contains("<name>A&amp;B &lt;cam&gt;</name>"));
        assert!(xml.contains("<keyMaterial>p&quot;w&apos;&lt;&amp;&gt;</keyMaterial>"));
        assert!(!xml.contains("A&B"));
    }

    #[test]
    fn reads_the_wifi_address_from_windows_ipconfig() {
        let listing = "Windows IP Configuration\r\n\r\nEthernet adapter Ethernet:\r\n\r\n   IPv4 Address. . . . . . . . . . . : 10.0.0.4\r\n\r\nWireless LAN adapter Wi-Fi:\r\n\r\n   Connection-specific DNS Suffix  . :\r\n   Link-local IPv6 Address . . . . . : fe80::1%12\r\n   IPv4 Address. . . . . . . . . . . : 192.168.122.10(Preferred)\r\n   Subnet Mask . . . . . . . . . . . : 255.255.255.0\r\n";
        assert_eq!(
            parse_ipconfig_address(listing, "Wi-Fi").as_deref(),
            Some("192.168.122.10")
        );
        assert_eq!(parse_ipconfig_address(listing, "Wi-Fi 2"), None);
        let disconnected = "Wireless LAN adapter Wi-Fi:\n\n   Media State . . . . . . . . . . . : Media disconnected\n\nEthernet adapter Ethernet:\n\n   IPv4 Address. . . . . . . . . . . : 10.0.0.4\n";
        assert_eq!(parse_ipconfig_address(disconnected, "Wi-Fi"), None);
        let self_assigned = "Wireless LAN adapter Wi-Fi:\n   Autoconfiguration IPv4 Address. . : 169.254.3.7(Preferred)\n";
        assert_eq!(parse_ipconfig_address(self_assigned, "Wi-Fi"), None);
    }

    #[test]
    fn the_profile_file_holds_the_xml_and_is_deleted_when_the_command_fails() {
        let seen = Mutex::new(None);
        let result: Result<(), WifiError> = with_profile_file("<WLANProfile/>", |path| {
            assert_eq!(std::fs::read_to_string(path).unwrap(), "<WLANProfile/>");
            assert_eq!(path.extension().and_then(|e| e.to_str()), Some("xml"));
            *seen.lock().unwrap() = Some(path.to_path_buf());
            Err(WifiError::Failed("connect failed".to_string()))
        });
        assert_eq!(result, Err(WifiError::Failed("connect failed".to_string())));
        let path = seen.lock().unwrap().clone().unwrap();
        assert!(!path.exists(), "{} was left behind", path.display());
    }

    #[test]
    fn the_profile_file_is_deleted_after_success_too() {
        let path = with_profile_file("<x/>", |path| Ok(path.to_path_buf())).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn netsh_key_value_arguments_quote_only_values_with_blanks() {
        assert_eq!(netsh_kv("interface", "Wi-Fi"), "interface=Wi-Fi");
        assert_eq!(netsh_kv("interface", "Wi-Fi 2"), "interface=\"Wi-Fi 2\"");
        assert_eq!(
            netsh_kv("filename", "C:\\Users\\John Smith\\rimix.xml"),
            "filename=\"C:\\Users\\John Smith\\rimix.xml\""
        );
        assert_eq!(netsh_kv("name", "tab\there"), "name=\"tab\there\"");
        assert_eq!(netsh_kv("name", "a\"b"), "name=a\\\"b");
        assert_eq!(netsh_kv("name", "a \"b"), "name=\"a \\\"b\"");
    }

    #[test]
    fn the_default_run_kv_passes_plain_key_value_arguments() {
        let runner = FakeRunner::scripted([Output {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        }]);
        runner
            .run_kv(
                "netsh",
                &["wlan", "connect"],
                &[("name", "My Cam"), ("ssid", "x")],
            )
            .unwrap();
        assert_eq!(
            runner.calls.lock().unwrap().clone(),
            vec![vec!["netsh", "wlan", "connect", "name=My Cam", "ssid=x"]]
        );
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
