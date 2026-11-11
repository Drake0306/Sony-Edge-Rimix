//! Finding cameras on the network before any PTP/IP connection.

use std::net::IpAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

use crate::error::Error;

pub(crate) mod http;
mod probe;
mod ssdp;
mod upnp;

pub use probe::probe_gateway;

/// The address of the network's router: on a camera's own Wi-Fi, the camera.
pub async fn gateway() -> Option<std::net::IpAddr> {
    probe::default_gateway().await.ok().flatten()
}
pub use ssdp::ssdp_search;
pub use upnp::{fetch_description, parse_dd_xml, parse_digital_imaging_desc};

/// The PTP/IP port Sony cameras listen on.
pub const PTP_PORT: u16 = 15740;

/// A camera found on the network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveredCamera {
    pub model: String,
    pub friendly_name: String,
    pub ip: IpAddr,
    pub ptp_port: u16,
    pub ptp_version: Option<String>,
    pub pairing_required: Option<bool>,
    pub ssh_supported: Option<bool>,
    pub source: DiscoverySource,
}

/// How a camera was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscoverySource {
    /// Answered an SSDP search and served a UPnP description.
    Ssdp,
    /// Accepted a connection on the PTP/IP port at the default gateway.
    Gateway,
    /// Entered by hand.
    Manual,
}

/// What one search of the network found.
#[derive(Debug, Clone, PartialEq)]
pub struct Discovery {
    pub cameras: Vec<DiscoveredCamera>,
    /// The operating system refused the search itself, so an empty list
    /// says nothing about the network: see [`local_network_blocked`].
    pub blocked: bool,
}

/// Finds cameras by SSDP and by probing the default gateway at the same
/// time, within `timeout` for each search.
///
/// Only Sony cameras are returned, one per IP address, preferring the SSDP
/// entry. A failing source is logged and skipped, so this does not fail.
pub async fn discover(timeout: Duration) -> Result<Vec<DiscoveredCamera>, Error> {
    Ok(discover_report(timeout).await?.cameras)
}

/// [`discover`], also saying whether the system refused the search.
pub async fn discover_report(timeout: Duration) -> Result<Discovery, Error> {
    let gateway = probe::default_gateway().await.unwrap_or_else(|e| {
        tracing::warn!(error = %e, "gateway probe failed");
        None
    });
    let (locations, at_gateway) = tokio::join!(
        ssdp_search(timeout, None),
        probe::probe_address(gateway, timeout)
    );
    let blocked =
        cfg!(target_os = "macos") && refused_by_system(gateway.is_some(), locations.as_ref().err());
    let locations = locations.unwrap_or_else(|e| {
        if blocked {
            tracing::warn!(
                "the system refused the SSDP search: this program is not allowed on the \
                 local network (System Settings, Privacy & Security, Local Network)"
            );
        } else {
            tracing::warn!(error = %e, "SSDP search failed");
        }
        Vec::new()
    });

    let mut fetches = JoinSet::new();
    for location in locations {
        fetches.spawn(async move {
            let result = fetch_description(&location).await;
            (location, result)
        });
    }
    let mut cameras: Vec<DiscoveredCamera> = Vec::new();
    while let Some(joined) = fetches.join_next().await {
        match joined {
            Ok((_, Ok(camera))) => cameras.push(camera),
            Ok((location, Err(e))) => {
                tracing::debug!(%location, error = %e, "skipping SSDP device")
            }
            Err(e) => tracing::warn!(error = %e, "description fetch task failed"),
        }
    }

    cameras.extend(at_gateway);
    Ok(Discovery {
        cameras: dedup_by_ip(cameras),
        blocked,
    })
}

/// What macOS gives a send it refuses: `EHOSTUNREACH`, "No route to host".
const MACOS_HOST_UNREACHABLE: i32 = 65;

/// Whether the operating system itself is keeping this program off the
/// local network, rather than the network being silent.
///
/// macOS asks the user once whether an app may reach devices on the local
/// network. Until that is allowed (and for a build macOS no longer matches
/// to the permission it stored) every send to the local network fails at
/// once with "No route to host", SSDP and the camera's PTP/IP port alike.
/// The same error is genuine when the computer has no default route, so
/// that case does not count. Always `false` off macOS.
pub async fn local_network_blocked() -> bool {
    // Sending is enough: a refusal comes back from the send itself.
    discover_report(Duration::ZERO)
        .await
        .is_ok_and(|found| found.blocked)
}

/// Whether a failed local send (`error`) on a computer that does or does
/// not have a default route was the system refusing it.
fn refused_by_system(has_default_route: bool, error: Option<&Error>) -> bool {
    has_default_route
        && matches!(error, Some(Error::Io(e)) if e.raw_os_error() == Some(MACOS_HOST_UNREACHABLE))
}

/// Keeps the first camera seen at each IP address.
fn dedup_by_ip(cameras: Vec<DiscoveredCamera>) -> Vec<DiscoveredCamera> {
    let mut seen = Vec::new();
    cameras
        .into_iter()
        .filter(|camera| {
            let fresh = !seen.contains(&camera.ip);
            seen.push(camera.ip);
            fresh
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn camera(model: &str, ip: [u8; 4], source: DiscoverySource) -> DiscoveredCamera {
        DiscoveredCamera {
            model: model.into(),
            friendly_name: model.into(),
            ip: IpAddr::V4(Ipv4Addr::from(ip)),
            ptp_port: PTP_PORT,
            ptp_version: None,
            pairing_required: None,
            ssh_supported: None,
            source,
        }
    }

    fn send_error(code: i32) -> Error {
        Error::Io(std::io::Error::from_raw_os_error(code))
    }

    #[test]
    fn a_refused_send_with_a_default_route_is_the_system_blocking() {
        assert!(refused_by_system(true, Some(&send_error(65))));
    }

    #[test]
    fn a_refused_send_without_a_default_route_is_just_no_route() {
        assert!(!refused_by_system(false, Some(&send_error(65))));
    }

    #[test]
    fn other_send_failures_and_successes_are_not_the_system_blocking() {
        // 51: network is unreachable, as while the Wi-Fi changes networks.
        assert!(!refused_by_system(true, Some(&send_error(51))));
        assert!(!refused_by_system(true, Some(&Error::Timeout)));
        assert!(!refused_by_system(true, None));
    }

    #[tokio::test]
    async fn an_address_with_nothing_to_probe_finds_nothing() {
        assert_eq!(
            probe::probe_address(None, Duration::from_millis(10)).await,
            None
        );
    }

    #[test]
    fn dedup_keeps_the_ssdp_entry_for_a_shared_ip() {
        let cameras = vec![
            camera("ZV-E10", [192, 168, 122, 1], DiscoverySource::Ssdp),
            camera("ILCE-7M4", [192, 168, 1, 20], DiscoverySource::Ssdp),
            camera("Camera", [192, 168, 122, 1], DiscoverySource::Gateway),
        ];
        let kept = dedup_by_ip(cameras);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].model, "ZV-E10");
        assert_eq!(kept[0].source, DiscoverySource::Ssdp);
        assert_eq!(kept[1].model, "ILCE-7M4");
    }
}
