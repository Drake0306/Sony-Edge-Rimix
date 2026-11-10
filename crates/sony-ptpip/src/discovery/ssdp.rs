//! SSDP M-SEARCH: asking UPnP devices on the network where their
//! descriptions live.

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::net::UdpSocket;
use url::Url;

use crate::error::Error;

/// The SSDP multicast group and port.
const SSDP_MULTICAST: SocketAddr = SocketAddr::new(
    std::net::IpAddr::V4(Ipv4Addr::new(239, 255, 255, 250)),
    1900,
);

const M_SEARCH: &str = "M-SEARCH * HTTP/1.1\r\n\
HOST: 239.255.255.250:1900\r\n\
MAN: \"ssdp:discover\"\r\n\
MX: 2\r\n\
ST: ssdp:all\r\n\r\n";

/// Sends one M-SEARCH to `target` (the SSDP multicast group when `None`) and
/// collects the distinct `LOCATION` URLs answered within `timeout`.
///
/// Every UPnP device answers; `fetch_description` tells cameras apart.
pub async fn ssdp_search(timeout: Duration, target: Option<SocketAddr>) -> Result<Vec<Url>, Error> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    socket.set_multicast_ttl_v4(2)?;
    socket
        .send_to(M_SEARCH.as_bytes(), target.unwrap_or(SSDP_MULTICAST))
        .await?;

    let deadline = tokio::time::Instant::now() + timeout;
    let mut locations = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let received = tokio::time::timeout_at(deadline, socket.recv_from(&mut buf)).await;
        let n = match received {
            Err(_) => break,
            Ok(Ok((n, _))) => n,
            // Windows reports an ICMP port-unreachable for the request as a
            // reset on the next receive; other responders may still answer.
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionReset => continue,
            Ok(Err(e)) => return Err(e.into()),
        };
        let Some(location) = parse_location(&String::from_utf8_lossy(&buf[..n])) else {
            continue;
        };
        if !locations.contains(&location) {
            locations.push(location);
        }
    }
    Ok(locations)
}

/// The `LOCATION` header of an SSDP response, if it holds a valid URL.
fn parse_location(response: &str) -> Option<Url> {
    response.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if !name.trim().eq_ignore_ascii_case("location") {
            return None;
        }
        Url::parse(value.trim()).ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_header_is_case_insensitive() {
        let response = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\n\
            Location: http://192.168.122.1:64321/dd.xml\r\nSERVER: UPnP/1.0 Sony\r\n\r\n";
        assert_eq!(
            parse_location(response).unwrap().as_str(),
            "http://192.168.122.1:64321/dd.xml"
        );
        assert_eq!(
            parse_location("HTTP/1.1 200 OK\r\nST: upnp:rootdevice\r\n\r\n"),
            None
        );
    }
}
