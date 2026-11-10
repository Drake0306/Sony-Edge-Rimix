//! The macOS command sequences: `networksetup` and `ipconfig`. Every command
//! is logged with its output; the password never is.

use crate::{
    parse_current_network, parse_wifi_device, redact, JoinReport, Output, Runner, Wifi, WifiError,
    FIND_RETRIES, FIND_RETRY_WAIT, JOIN_POLL, JOIN_WAIT,
};

/// What `networksetup -setairportnetwork` prints, with exit status 0, when the
/// network is not in the Wi-Fi scan cache yet.
const NOT_FOUND: &str = "Could not find network";

impl Wifi {
    /// Joins `ssid`. Success is the network name reading back, or the Wi-Fi
    /// interface getting an address while no other network name reads back:
    /// recent macOS often will not print the current network name even
    /// while joined.
    pub(crate) fn system_join(
        &self,
        runner: &dyn Runner,
        ssid: &str,
        password: Option<&str>,
    ) -> Result<JoinReport, WifiError> {
        let device = system_device(runner)?;
        if network_on(runner, &device)?.as_deref() == Some(ssid) {
            return Ok(JoinReport {
                attempts: 0,
                seen_ssid: Some(ssid.to_string()),
                address: None,
            });
        }
        let mut args = vec!["networksetup", "-setairportnetwork", device.as_str(), ssid];
        let mut secret = Vec::new();
        if let Some(p) = password.filter(|p| !p.is_empty()) {
            secret.push(args.len());
            args.push(p);
        }
        let mut attempts = 0;
        loop {
            attempts += 1;
            let out = run_logged(runner, &args, &secret)?;
            if out.stdout.contains(NOT_FOUND) {
                if attempts >= FIND_RETRIES {
                    return Err(WifiError::NotVisible(ssid.to_string()));
                }
                (self.sleep)(FIND_RETRY_WAIT);
                continue;
            }
            if !out.success || out.stdout.contains("Failed") || out.stdout.contains("Could not") {
                let text = format!("{}{}", out.stdout, out.stderr);
                return Err(WifiError::Failed(text.trim().to_string()));
            }
            break;
        }
        let deadline = (self.now)() + JOIN_WAIT;
        loop {
            match network_on(runner, &device)? {
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
                    if let Some(address) = address_on(runner, &device) {
                        return Ok(JoinReport {
                            attempts,
                            seen_ssid: None,
                            address: Some(address),
                        });
                    }
                }
            }
            if (self.now)() >= deadline {
                return Err(WifiError::Failed(format!("{ssid} did not come up")));
            }
            (self.sleep)(JOIN_POLL);
        }
    }
}

/// Runs `args[0]` with the rest, logging the command line (secrets replaced)
/// and its trimmed output at `info`.
fn run_logged(runner: &dyn Runner, args: &[&str], secret: &[usize]) -> Result<Output, WifiError> {
    run_with_log(runner, args, secret, false)
}

/// Runs a command and logs it; a `long_output` is logged at `debug` only.
fn run_with_log(
    runner: &dyn Runner,
    args: &[&str],
    secret: &[usize],
    long_output: bool,
) -> Result<Output, WifiError> {
    let cmd = redact(args, secret);
    match runner.run(args[0], &args[1..]) {
        Ok(out) => {
            let text = format!("{}{}", out.stdout, out.stderr);
            if long_output {
                tracing::debug!(cmd = %cmd, output = %text.trim(), "{}", args[0]);
            } else {
                tracing::info!(cmd = %cmd, output = %text.trim(), "{}", args[0]);
            }
            Ok(out)
        }
        Err(e) => {
            tracing::warn!(cmd = %cmd, error = %e, "{}", args[0]);
            Err(WifiError::Failed(format!("Could not run {}: {e}", args[0])))
        }
    }
}

pub(crate) fn system_device(runner: &dyn Runner) -> Result<String, WifiError> {
    let out = run_with_log(
        runner,
        &["networksetup", "-listallhardwareports"],
        &[],
        true,
    )?;
    let device = parse_wifi_device(&out.stdout);
    match &device {
        Some(device) => tracing::info!(device = %device, "Wi-Fi device"),
        None => tracing::info!("no Wi-Fi device in the hardware port listing"),
    }
    device.ok_or(WifiError::NoWifi)
}

pub(crate) fn system_current_network(runner: &dyn Runner) -> Result<Option<String>, WifiError> {
    let device = system_device(runner)?;
    network_on(runner, &device)
}

pub(crate) fn system_address(runner: &dyn Runner) -> Result<Option<String>, WifiError> {
    let device = system_device(runner)?;
    Ok(address_on(runner, &device))
}

fn network_on(runner: &dyn Runner, device: &str) -> Result<Option<String>, WifiError> {
    let out = run_logged(runner, &["networksetup", "-getairportnetwork", device], &[])?;
    Ok(parse_current_network(&out.stdout))
}

/// The IPv4 address `ipconfig getifaddr` prints for `device`, if any.
fn address_on(runner: &dyn Runner, device: &str) -> Option<String> {
    let out = run_logged(runner, &["ipconfig", "getifaddr", device], &[]).ok()?;
    let text = out.stdout.trim();
    (out.success && text.parse::<std::net::Ipv4Addr>().is_ok()).then(|| text.to_string())
}
