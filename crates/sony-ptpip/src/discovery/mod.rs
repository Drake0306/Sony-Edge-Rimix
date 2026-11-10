//! Finding cameras on the network before any PTP/IP connection.

use std::net::IpAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

use crate::error::Error;

mod http;
mod probe;
mod ssdp;
mod upnp;

pub use probe::probe_gateway;
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

/// Finds cameras by SSDP and by probing the default gateway at the same
/// time, within `timeout` for each search.
///
/// Only Sony cameras are returned, one per IP address, preferring the SSDP
/// entry. A failing source is logged and skipped, so this does not fail.
pub async fn discover(timeout: Duration) -> Result<Vec<DiscoveredCamera>, Error> {
    let (locations, gateway) = tokio::join!(ssdp_search(timeout, None), probe_gateway(timeout));
    let locations = locations.unwrap_or_else(|e| {
        tracing::warn!(error = %e, "SSDP search failed");
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

    match gateway {
        Ok(Some(camera)) => cameras.push(camera),
        Ok(None) => {}
        Err(e) => tracing::warn!(error = %e, "gateway probe failed"),
    }
    Ok(dedup_by_ip(cameras))
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
