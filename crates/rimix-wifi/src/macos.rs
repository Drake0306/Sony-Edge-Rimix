//! The macOS command sequences: `networksetup` and `ipconfig`.

use crate::{
    parse_current_network, parse_wifi_device, Runner, Wifi, WifiError, JOIN_POLL, JOIN_WAIT,
};

impl Wifi {
    /// Joins `ssid`. Success is the network name reading back, or the Wi-Fi
    /// interface getting an address: recent macOS often will not print the
    /// current network name even while joined.
    pub(crate) fn system_join(
        &self,
        runner: &dyn Runner,
        ssid: &str,
        password: Option<&str>,
    ) -> Result<(), WifiError> {
        let device = system_device(runner)?;
        if network_on(runner, &device)?.as_deref() == Some(ssid) {
            return Ok(());
        }
        let mut args = vec!["-setairportnetwork", device.as_str(), ssid];
        if let Some(p) = password.filter(|p| !p.is_empty()) {
            args.push(p);
        }
        let out = runner
            .run("networksetup", &args)
            .map_err(|e| WifiError::Failed(format!("Could not run networksetup: {e}")))?;
        if !out.success || out.stdout.contains("Failed") || out.stdout.contains("Could not") {
            let text = format!("{}{}", out.stdout, out.stderr);
            return Err(WifiError::Failed(text.trim().to_string()));
        }
        let deadline = (self.now)() + JOIN_WAIT;
        loop {
            if network_on(runner, &device)?.as_deref() == Some(ssid) {
                return Ok(());
            }
            if has_address(runner, &device) {
                return Ok(());
            }
            if (self.now)() >= deadline {
                return Err(WifiError::Failed(format!("{ssid} did not come up")));
            }
            (self.sleep)(JOIN_POLL);
        }
    }
}

pub(crate) fn system_device(runner: &dyn Runner) -> Result<String, WifiError> {
    let out = runner
        .run("networksetup", &["-listallhardwareports"])
        .map_err(|e| WifiError::Failed(format!("Could not run networksetup: {e}")))?;
    parse_wifi_device(&out.stdout).ok_or(WifiError::NoWifi)
}

pub(crate) fn system_current_network(runner: &dyn Runner) -> Result<Option<String>, WifiError> {
    let device = system_device(runner)?;
    network_on(runner, &device)
}

fn network_on(runner: &dyn Runner, device: &str) -> Result<Option<String>, WifiError> {
    let out = runner
        .run("networksetup", &["-getairportnetwork", device])
        .map_err(|e| WifiError::Failed(format!("Could not run networksetup: {e}")))?;
    Ok(parse_current_network(&out.stdout))
}

/// Whether `ipconfig getifaddr` prints an IPv4 address for `device`.
fn has_address(runner: &dyn Runner, device: &str) -> bool {
    match runner.run("ipconfig", &["getifaddr", device]) {
        Ok(out) => out.success && out.stdout.trim().parse::<std::net::Ipv4Addr>().is_ok(),
        Err(_) => false,
    }
}
