//! Finding a camera at the default gateway, as when the computer joins the
//! camera's own access point.

use std::net::IpAddr;
use std::time::Duration;

use tokio::net::TcpStream;

use super::{DiscoveredCamera, DiscoverySource, PTP_PORT};
use crate::error::Error;

/// Looks up the default gateway and tries a TCP connection to its PTP/IP
/// port. A camera acting as the access point is usually the gateway.
///
/// Returns `Ok(None)` when there is no gateway, nothing answers within
/// `timeout`, or the platform is not macOS or Windows.
pub async fn probe_gateway(timeout: Duration) -> Result<Option<DiscoveredCamera>, Error> {
    Ok(probe_address(default_gateway().await?, timeout).await)
}

/// Tries a TCP connection to the PTP/IP port of the gateway at `ip`.
/// `None` when there is no gateway or nothing answers within `timeout`.
pub(super) async fn probe_address(
    ip: Option<IpAddr>,
    timeout: Duration,
) -> Option<DiscoveredCamera> {
    let ip = ip?;
    match tokio::time::timeout(timeout, TcpStream::connect((ip, PTP_PORT))).await {
        Ok(Ok(_stream)) => Some(DiscoveredCamera {
            model: "Camera".into(),
            friendly_name: format!("Camera at {ip}"),
            ip,
            ptp_port: PTP_PORT,
            ptp_version: None,
            pairing_required: None,
            ssh_supported: None,
            source: DiscoverySource::Gateway,
        }),
        Ok(Err(e)) => {
            tracing::debug!(%ip, error = %e, "gateway refused PTP/IP");
            None
        }
        Err(_) => None,
    }
}

#[cfg(target_os = "macos")]
pub(super) async fn default_gateway() -> Result<Option<IpAddr>, Error> {
    let mut command = std::process::Command::new("route");
    command.args(["-n", "get", "default"]);
    let output = run(command).await?;
    Ok(parse_macos_route_output(&output))
}

#[cfg(target_os = "windows")]
pub(super) async fn default_gateway() -> Result<Option<IpAddr>, Error> {
    use std::os::windows::process::CommandExt;
    /// Keeps a console window from flashing up in GUI apps.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = std::process::Command::new("route");
    command
        .args(["print", "0.0.0.0"])
        .creation_flags(CREATE_NO_WINDOW);
    let output = run(command).await?;
    Ok(parse_windows_route_output(&output))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(super) async fn default_gateway() -> Result<Option<IpAddr>, Error> {
    Ok(None)
}

/// Runs `command` off the async threads and returns its standard output.
#[cfg(any(target_os = "macos", target_os = "windows"))]
async fn run(mut command: std::process::Command) -> Result<String, Error> {
    let output = tokio::task::spawn_blocking(move || command.output())
        .await
        .map_err(|e| Error::Io(std::io::Error::other(e)))??;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The `gateway:` line of `route -n get default`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_macos_route_output(output: &str) -> Option<IpAddr> {
    output.lines().find_map(|line| {
        let value = line.trim().strip_prefix("gateway:")?;
        value.trim().parse().ok()
    })
}

/// The gateway column of the first usable `0.0.0.0` row of
/// `route print 0.0.0.0`; `On-link` rows are skipped.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn parse_windows_route_output(output: &str) -> Option<IpAddr> {
    output.lines().find_map(|line| {
        let columns: Vec<&str> = line.split_whitespace().collect();
        match columns.as_slice() {
            ["0.0.0.0", "0.0.0.0", gateway, ..] => gateway.parse().ok(),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    const MACOS_ROUTE: &str = "   route to: default
destination: default
       mask: default
    gateway: 192.168.122.1
  interface: en0
      flags: <UP,GATEWAY,DONE,STATIC,PRCLONING,GLOBAL>
 recvpipe  sendpipe  ssthresh  rtt,msec    rttvar  hopcount      mtu     expire
       0         0         0         0         0         0      1500         0
";

    const WINDOWS_ROUTE: &str =
        "===========================================================================
Interface List
 12...a4 c3 f0 11 22 33 ......Intel(R) Wi-Fi 6 AX201 160MHz
  1...........................Software Loopback Interface 1
===========================================================================

IPv4 Route Table
===========================================================================
Active Routes:
Network Destination        Netmask          Gateway       Interface  Metric
          0.0.0.0          0.0.0.0    192.168.122.1  192.168.122.50     35
===========================================================================
Persistent Routes:
  None
";

    #[test]
    fn parses_macos_gateway() {
        assert_eq!(
            parse_macos_route_output(MACOS_ROUTE),
            Some(IpAddr::V4(Ipv4Addr::new(192, 168, 122, 1)))
        );
    }

    #[test]
    fn macos_output_without_gateway_gives_none() {
        let out = "   route to: default\ndestination: default\n  interface: utun3\n";
        assert_eq!(parse_macos_route_output(out), None);
        assert_eq!(
            parse_macos_route_output("route: writing to routing socket: not in table\n"),
            None
        );
    }

    #[test]
    fn parses_windows_gateway() {
        assert_eq!(
            parse_windows_route_output(WINDOWS_ROUTE),
            Some(IpAddr::V4(Ipv4Addr::new(192, 168, 122, 1)))
        );
    }

    #[test]
    fn windows_on_link_rows_are_skipped() {
        let out = "Network Destination        Netmask          Gateway       Interface  Metric
          0.0.0.0          0.0.0.0         On-link     10.0.0.5     5
          0.0.0.0          0.0.0.0        10.0.0.1     10.0.0.5    25
";
        assert_eq!(
            parse_windows_route_output(out),
            Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)))
        );
        assert_eq!(
            parse_windows_route_output("Persistent Routes:\n  None\n"),
            None
        );
    }
}
