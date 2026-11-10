//! The Windows command sequences: `netsh wlan` and `ipconfig`. Every command
//! is logged with its output. The password only ever goes into a temporary
//! profile file, which is deleted as soon as `netsh` has read it and is
//! never logged; no command line carries it.

use std::sync::atomic::AtomicBool;

use crate::run::{run_kv_logged, run_with_log};
use crate::{
    needs_location_permission, parse_interfaces, parse_ipconfig_address, profile_xml,
    with_profile_file, Interface, JoinReport, Runner, Wifi, WifiError, FIND_RETRIES,
    FIND_RETRY_WAIT, JOIN_POLL, JOIN_WAIT, LOCATION_NEEDED,
};

/// What `netsh wlan connect` prints when Windows cannot see the network yet
/// or has no profile for it. `is not available` covers both
/// `The network specified is not available` and, once a profile exists,
/// `The network specified by profile "<name>" is not available to connect.`
const NOT_VISIBLE: &[&str] = &[
    "There is no profile",
    "is not available",
    "There is no such wireless network interface",
];

const SHOW_INTERFACES: &[&str] = &["netsh", "wlan", "show", "interfaces"];

impl Wifi {
    /// Joins `ssid`. With a password, a WPA2-Personal profile is added first
    /// and kept, so Windows can rejoin later without it. Success is the
    /// interface reading back `connected` on `ssid`, or `connected` with an
    /// address while no network name reads back.
    pub(crate) fn system_join(
        &self,
        runner: &dyn Runner,
        ssid: &str,
        password: Option<&str>,
        cancel: &AtomicBool,
    ) -> Result<JoinReport, WifiError> {
        let wifi = first_interface(runner)?.ok_or(WifiError::NoWifi)?;
        if wifi.is_connected() && wifi.ssid.as_deref() == Some(ssid) {
            return Ok(JoinReport {
                attempts: 0,
                seen_ssid: Some(ssid.to_string()),
                address: None,
            });
        }
        if let Some(password) = password.filter(|p| !p.is_empty()) {
            add_profile(runner, ssid, password)?;
        }
        let connect = [
            ("name", ssid),
            ("ssid", ssid),
            ("interface", wifi.name.as_str()),
        ];
        let mut attempts = 0;
        loop {
            attempts += 1;
            let out = run_kv_logged(runner, &["netsh", "wlan", "connect"], &connect)?;
            let text = format!("{}{}", out.stdout, out.stderr);
            if NOT_VISIBLE.iter().any(|t| text.contains(t)) {
                if attempts >= FIND_RETRIES {
                    return Err(WifiError::NotVisible(ssid.to_string()));
                }
                self.pause(FIND_RETRY_WAIT, cancel)?;
                continue;
            }
            if !out.success {
                return Err(WifiError::Failed(text.trim().to_string()));
            }
            break;
        }
        let deadline = (self.now)() + JOIN_WAIT;
        loop {
            if let Some(now) = interface_named(runner, &wifi.name)? {
                if now.is_connected() {
                    match now.ssid {
                        Some(name) if name == ssid => {
                            return Ok(JoinReport {
                                attempts,
                                seen_ssid: Some(name),
                                address: None,
                            });
                        }
                        // Still on another network: any address is that network's.
                        Some(_) => {}
                        None => {
                            if let Some(address) = address_on(runner, &wifi.name) {
                                return Ok(JoinReport {
                                    attempts,
                                    seen_ssid: None,
                                    address: Some(address),
                                });
                            }
                        }
                    }
                }
            }
            if (self.now)() >= deadline {
                return Err(WifiError::Failed(format!("{ssid} did not come up")));
            }
            self.pause(JOIN_POLL, cancel)?;
        }
    }
}

/// Adds a WPA2-Personal profile for `ssid` from a temporary file that is
/// deleted again whether or not `netsh` accepted it.
fn add_profile(runner: &dyn Runner, ssid: &str, password: &str) -> Result<(), WifiError> {
    with_profile_file(&profile_xml(ssid, password), |path| {
        let filename = path.to_string_lossy();
        let out = run_kv_logged(
            runner,
            &["netsh", "wlan", "add", "profile"],
            &[("filename", &filename), ("user", "current")],
        )?;
        if out.success {
            Ok(())
        } else {
            let text = format!("{}{}", out.stdout, out.stderr);
            Err(WifiError::Failed(text.trim().to_string()))
        }
    })
}

fn interfaces(runner: &dyn Runner) -> Result<Vec<Interface>, WifiError> {
    let out = run_with_log(runner, SHOW_INTERFACES, &[], true)?;
    if needs_location_permission(&format!("{}{}", out.stdout, out.stderr)) {
        tracing::info!("Windows withheld the Wi-Fi interfaces: Location is off for desktop apps");
        return Err(WifiError::Failed(LOCATION_NEEDED.to_string()));
    }
    Ok(parse_interfaces(&out.stdout))
}

fn first_interface(runner: &dyn Runner) -> Result<Option<Interface>, WifiError> {
    Ok(interfaces(runner)?.into_iter().next())
}

fn interface_named(runner: &dyn Runner, name: &str) -> Result<Option<Interface>, WifiError> {
    Ok(interfaces(runner)?.into_iter().find(|i| i.name == name))
}

pub(crate) fn system_device(runner: &dyn Runner) -> Result<String, WifiError> {
    let device = first_interface(runner)?.map(|i| i.name);
    match &device {
        Some(device) => tracing::info!(device = %device, "Wi-Fi device"),
        None => tracing::info!("no wireless interface in netsh wlan show interfaces"),
    }
    device.ok_or(WifiError::NoWifi)
}

pub(crate) fn system_current_network(runner: &dyn Runner) -> Result<Option<String>, WifiError> {
    Ok(first_interface(runner)?
        .filter(Interface::is_connected)
        .and_then(|i| i.ssid))
}

pub(crate) fn system_address(runner: &dyn Runner) -> Result<Option<String>, WifiError> {
    let device = system_device(runner)?;
    Ok(address_on(runner, &device))
}

/// The IPv4 address `ipconfig` shows for the adapter `interface`, if any.
fn address_on(runner: &dyn Runner, interface: &str) -> Option<String> {
    let out = run_with_log(runner, &["ipconfig"], &[], true).ok()?;
    if !out.success {
        return None;
    }
    parse_ipconfig_address(&out.stdout, interface)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::FakeRunner;
    use crate::Output;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    const SSID: &str = "DIRECT-abc1:ZV-E10";

    fn out(stdout: &str) -> Output {
        Output {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    fn failed(stdout: &str) -> Output {
        Output {
            success: false,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    fn on(name: &str) -> Output {
        out(&format!(
            "There is 1 interface on the system:\r\n\r\n    Name                   : Wi-Fi\r\n    State                  : connected\r\n    SSID                   : {name}\r\n    Profile                : {name}\r\n"
        ))
    }

    fn connected_without_name() -> Output {
        out("There is 1 interface on the system:\r\n\r\n    Name                   : Wi-Fi\r\n    State                  : connected\r\n")
    }

    fn off() -> Output {
        out("There is 1 interface on the system:\r\n\r\n    Name                   : Wi-Fi\r\n    State                  : disconnected\r\n")
    }

    fn ipconfig(address: &str) -> Output {
        out(&format!(
            "Windows IP Configuration\r\n\r\nWireless LAN adapter Wi-Fi:\r\n\r\n   IPv4 Address. . . . . . . . . . . : {address}\r\n"
        ))
    }

    fn added() -> Output {
        out(&format!("Profile {SSID} is added on interface Wi-Fi.\r\n"))
    }

    fn requested() -> Output {
        out("Connection request was completed successfully.\r\n")
    }

    fn not_visible() -> Output {
        failed("The network specified by profile \"DIRECT-abc1:ZV-E10\" is not available to connect.\r\n")
    }

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| p.to_string()).collect()
    }

    fn connect_argv() -> Vec<String> {
        argv(&[
            "netsh",
            "wlan",
            "connect",
            &format!("name={SSID}"),
            &format!("ssid={SSID}"),
            "interface=Wi-Fi",
        ])
    }

    fn is_add_profile(call: &[String]) -> bool {
        call.len() == 6
            && call[..4] == argv(&["netsh", "wlan", "add", "profile"])
            && call[4].starts_with("filename=")
            && call[4].ends_with(".xml")
            && call[5] == "user=current"
    }

    fn profile_path(call: &[String]) -> PathBuf {
        PathBuf::from(call[4].strip_prefix("filename=").unwrap())
    }

    /// A Wi-Fi whose clock moves only when it sleeps.
    fn wifi_with_fake_clock(runner: Arc<FakeRunner>) -> Wifi {
        let start = Instant::now();
        let elapsed = Arc::new(Mutex::new(Duration::ZERO));
        let read = elapsed.clone();
        Wifi::new(runner).with_clock(
            move || start + *read.lock().unwrap(),
            move |d| *elapsed.lock().unwrap() += d,
        )
    }

    #[test]
    fn device_is_the_first_interface_name() {
        let wifi = Wifi::new(FakeRunner::scripted([on("Home")]));
        assert_eq!(wifi.device().unwrap(), "Wi-Fi");
        let wifi = Wifi::new(FakeRunner::scripted([failed(
            "The Wireless AutoConfig Service (wlansvc) is not running.\r\n",
        )]));
        assert_eq!(wifi.device(), Err(WifiError::NoWifi));
    }

    #[test]
    fn current_network_is_the_ssid_only_while_connected() {
        let wifi = Wifi::new(FakeRunner::scripted([on(SSID), off()]));
        assert_eq!(wifi.current_network().unwrap().as_deref(), Some(SSID));
        assert_eq!(wifi.current_network().unwrap(), None);
    }

    #[test]
    fn already_joined_runs_nothing_more() {
        let runner = FakeRunner::scripted([on(SSID)]);
        let wifi = wifi_with_fake_clock(runner.clone());
        assert_eq!(
            wifi.join_report(SSID, Some("12345678")).unwrap().attempts,
            0
        );
        assert_eq!(runner.calls(), vec![argv(SHOW_INTERFACES)]);
    }

    #[test]
    fn join_adds_a_profile_from_a_deleted_temp_file_then_connects() {
        let runner = FakeRunner::scripted([on("Home"), added(), requested(), off(), on(SSID)]);
        let wifi = wifi_with_fake_clock(runner.clone());
        let report = wifi.join_report(SSID, Some("12345678")).unwrap();
        assert_eq!(report.attempts, 1);
        assert_eq!(report.seen_ssid.as_deref(), Some(SSID));
        let calls = runner.calls();
        assert_eq!(calls.len(), 5);
        assert_eq!(calls[0], argv(SHOW_INTERFACES));
        assert!(is_add_profile(&calls[1]), "{:?}", calls[1]);
        assert!(
            !profile_path(&calls[1]).exists(),
            "the profile file was left behind"
        );
        assert_eq!(calls[2], connect_argv());
        assert_eq!(calls[3], argv(SHOW_INTERFACES));
        assert_eq!(calls[4], argv(SHOW_INTERFACES));
        assert!(
            calls.iter().flatten().all(|a| !a.contains("12345678")),
            "the password reached a command line"
        );
    }

    #[test]
    fn the_profile_file_is_gone_after_a_failing_connect() {
        let runner = FakeRunner::scripted([
            off(),
            added(),
            failed("The request was rejected by the network.\r\n"),
        ]);
        let wifi = wifi_with_fake_clock(runner.clone());
        let err = wifi.join(SSID, Some("12345678")).unwrap_err();
        assert!(matches!(err, WifiError::Failed(ref t) if t.contains("rejected")));
        let calls = runner.calls();
        assert!(is_add_profile(&calls[1]));
        assert!(!profile_path(&calls[1]).exists());
    }

    #[test]
    fn the_profile_file_is_gone_when_netsh_rejects_the_profile() {
        let runner = FakeRunner::scripted([off(), failed("The profile format is invalid.\r\n")]);
        let wifi = wifi_with_fake_clock(runner.clone());
        let err = wifi.join(SSID, Some("12345678")).unwrap_err();
        assert_eq!(
            err,
            WifiError::Failed("The profile format is invalid.".to_string())
        );
        let calls = runner.calls();
        assert_eq!(calls.len(), 2, "no connect after a rejected profile");
        assert!(!profile_path(&calls[1]).exists());
    }

    #[test]
    fn join_without_password_skips_the_profile() {
        for password in [None, Some("")] {
            let runner = FakeRunner::scripted([off(), requested(), on(SSID)]);
            let wifi = wifi_with_fake_clock(runner.clone());
            wifi.join(SSID, password).unwrap();
            assert_eq!(
                runner.calls(),
                vec![argv(SHOW_INTERFACES), connect_argv(), argv(SHOW_INTERFACES)]
            );
        }
    }

    #[test]
    fn join_retries_while_the_network_is_not_visible() {
        let runner = FakeRunner::scripted([
            off(),
            added(),
            not_visible(),
            failed("There is no profile \"DIRECT-abc1:ZV-E10\" assigned to the specified interface.\r\n"),
            requested(),
            on(SSID),
        ]);
        let wifi = wifi_with_fake_clock(runner.clone());
        let report = wifi.join_report(SSID, Some("pw")).unwrap();
        assert_eq!(report.attempts, 3);
        let calls = runner.calls();
        assert_eq!(calls.iter().filter(|c| is_add_profile(c)).count(), 1);
        assert_eq!(calls.iter().filter(|c| **c == connect_argv()).count(), 3);
    }

    #[test]
    fn join_gives_not_visible_after_the_last_retry() {
        // Both wordings: without a usable profile, and with one.
        let mut script = vec![
            off(),
            failed("The network specified is not available. Please check the network name.\r\n"),
        ];
        script.extend((1..FIND_RETRIES).map(|_| not_visible()));
        let runner = FakeRunner::scripted(script);
        let slept = Arc::new(Mutex::new(Vec::new()));
        let record = slept.clone();
        let wifi = Wifi::new(runner.clone())
            .with_clock(Instant::now, move |d| record.lock().unwrap().push(d));
        let err = wifi.join(SSID, None).unwrap_err();
        assert_eq!(err, WifiError::NotVisible(SSID.to_string()));
        assert_eq!(
            *slept.lock().unwrap(),
            vec![FIND_RETRY_WAIT; FIND_RETRIES as usize - 1]
        );
        assert_eq!(runner.calls().len(), 1 + FIND_RETRIES as usize);
    }

    #[test]
    fn join_succeeds_on_an_address_when_no_name_reads_back() {
        let runner = FakeRunner::scripted([
            off(),
            requested(),
            connected_without_name(),
            ipconfig("169.254.3.7"),
            connected_without_name(),
            ipconfig("192.168.122.10"),
        ]);
        let wifi = wifi_with_fake_clock(runner.clone());
        let report = wifi.join_report(SSID, None).unwrap();
        assert_eq!(report.address.as_deref(), Some("192.168.122.10"));
        assert_eq!(report.seen_ssid, None);
        assert_eq!(runner.calls()[3], argv(&["ipconfig"]));
    }

    #[test]
    fn join_gives_up_when_the_network_never_comes_up() {
        // Polls at 0 s, 1 s, .. 20 s; still on the old network, so no address read.
        let polls = JOIN_WAIT.as_secs() as usize + 1;
        let mut script = vec![off(), requested()];
        script.extend((0..polls).map(|_| on("Home")));
        let runner = FakeRunner::scripted(script);
        let wifi = wifi_with_fake_clock(runner.clone());
        let err = wifi.join(SSID, None).unwrap_err();
        assert_eq!(err, WifiError::Failed(format!("{SSID} did not come up")));
        assert!(!runner.calls().contains(&argv(&["ipconfig"])));
        assert_eq!(runner.calls().len(), 2 + polls);
    }

    #[test]
    fn missing_location_permission_says_what_to_turn_on() {
        let wifi = Wifi::new(FakeRunner::scripted([failed(
            "Network shell commands need location permission to access WLAN information.\r\n",
        )]));
        assert_eq!(
            wifi.join(SSID, Some("pw")),
            Err(WifiError::Failed(LOCATION_NEEDED.to_string()))
        );
    }

    #[test]
    fn a_cancel_during_the_retry_wait_stops_the_join() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let runner = FakeRunner::scripted([off(), not_visible()]);
        let cancel = Arc::new(AtomicBool::new(false));
        let set = cancel.clone();
        let wifi = Wifi::new(runner.clone())
            .with_clock(Instant::now, move |_| set.store(true, Ordering::SeqCst));
        let err = wifi.join_with_cancel(SSID, None, &cancel).unwrap_err();
        assert_eq!(err, WifiError::Failed(crate::CANCELLED.to_string()));
        assert_eq!(runner.calls(), vec![argv(SHOW_INTERFACES), connect_argv()]);
    }

    #[test]
    fn current_address_reads_ipconfig() {
        let runner = FakeRunner::scripted([on(SSID), ipconfig("192.168.122.10")]);
        let wifi = Wifi::new(runner);
        assert_eq!(
            wifi.current_address().unwrap(),
            Some(std::net::Ipv4Addr::new(192, 168, 122, 10))
        );
    }

    #[test]
    fn no_wireless_interface_is_a_plain_error() {
        let wifi = Wifi::new(FakeRunner::scripted([out(
            "There is 0 interface on the system:\r\n",
        )]));
        assert_eq!(wifi.join(SSID, Some("pw")), Err(WifiError::NoWifi));
    }
}
