//! UPnP device descriptions: `dd.xml` and Sony's `DigitalImagingDesc.xml`.

use std::net::IpAddr;

use quick_xml::events::Event;
use quick_xml::Reader;
use url::{Host, Url};

use super::{http, DiscoveredCamera, DiscoverySource, PTP_PORT};
use crate::error::Error;

/// Fetches the UPnP description at `location` and, when it links one, Sony's
/// imaging description with the PTP/IP flags.
///
/// Devices that are not Sony cameras give `Error::Unsupported`.
pub async fn fetch_description(location: &Url) -> Result<DiscoveredCamera, Error> {
    let ip = host_ip(location).await?;
    let (model, friendly_name, manufacturer, desc_url) = parse_dd_xml(&http::get(location).await?)?;
    if !is_sony_camera(&manufacturer, &model) {
        return Err(Error::Unsupported("not a Sony camera"));
    }
    let (ptp_version, pairing_required, ssh_supported) = match desc_url {
        Some(path) => {
            let url = location.join(&path).map_err(|e| {
                Error::Protocol(format!("bad imaging description URL {path:?}: {e}"))
            })?;
            parse_digital_imaging_desc(&http::get(&url).await?)?
        }
        None => (None, None, None),
    };
    Ok(DiscoveredCamera {
        model,
        friendly_name,
        ip,
        ptp_port: PTP_PORT,
        ptp_version,
        pairing_required,
        ssh_supported,
        source: DiscoverySource::Ssdp,
    })
}

fn is_sony_camera(manufacturer: &str, model: &str) -> bool {
    manufacturer.contains("Sony")
        || ["ILCE", "ZV-", "DSC", "ILME"]
            .iter()
            .any(|prefix| model.starts_with(prefix))
}

async fn host_ip(url: &Url) -> Result<IpAddr, Error> {
    match url.host() {
        Some(Host::Ipv4(ip)) => Ok(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => Ok(IpAddr::V6(ip)),
        Some(Host::Domain(name)) => tokio::net::lookup_host((name, 0))
            .await?
            .next()
            .map(|addr| addr.ip())
            .ok_or_else(|| Error::Protocol(format!("{name} has no address"))),
        None => Err(Error::Protocol(format!("{url} has no host"))),
    }
}

/// Reads a UPnP `dd.xml`: model name, friendly name (the model when absent),
/// manufacturer (empty when absent) and the imaging description link, if any.
pub fn parse_dd_xml(xml: &str) -> Result<(String, String, String, Option<String>), Error> {
    let elements = leaf_elements(xml)?;
    let find = |name: &str| {
        elements
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, text)| text.clone())
    };
    let model = find("modelName")
        .ok_or_else(|| Error::Protocol("device description has no modelName".into()))?;
    let friendly = find("friendlyName").unwrap_or_else(|| model.clone());
    let manufacturer = find("manufacturer").unwrap_or_default();
    let desc = elements
        .iter()
        .find(|(n, _)| n.ends_with("DigitalImagingDescURL"))
        .map(|(_, text)| text.clone());
    Ok((model, friendly, manufacturer, desc))
}

/// Reads Sony's `DigitalImagingDesc.xml`: PTP version, whether pairing is
/// needed and whether SSH is supported, each when present.
// The tuple is the agreed interface; a struct would be clearer but differ.
#[allow(clippy::type_complexity)]
pub fn parse_digital_imaging_desc(
    xml: &str,
) -> Result<(Option<String>, Option<bool>, Option<bool>), Error> {
    let elements = leaf_elements(xml)?;
    let find = |name: &str| {
        elements
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, text)| text.as_str())
    };
    Ok((
        find("X_PTP_Versions").map(str::to_owned),
        find("X_PTP_PairingNecessity").and_then(parse_flag),
        find("X_SSH_Support").and_then(parse_flag),
    ))
}

fn parse_flag(text: &str) -> Option<bool> {
    match text.to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// Every element's local name with its trimmed text, in document order;
/// elements with no text are left out.
fn leaf_elements(xml: &str) -> Result<Vec<(String, String)>, Error> {
    let bad = |e: &dyn std::fmt::Display| Error::Protocol(format!("bad XML: {e}"));
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<String> = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event().map_err(|e| bad(&e))? {
            Event::Start(e) => {
                stack.push(String::from_utf8_lossy(e.local_name().as_ref()).into_owned())
            }
            Event::End(_) => {
                stack.pop();
            }
            Event::Text(t) => {
                if let Some(name) = stack.last() {
                    let text = t.unescape().map_err(|e| bad(&e))?;
                    out.push((name.clone(), text.trim().to_owned()));
                }
            }
            Event::CData(t) => {
                if let Some(name) = stack.last() {
                    let text = t.decode().map_err(|e| bad(&e))?;
                    out.push((name.clone(), text.trim().to_owned()));
                }
            }
            Event::Eof if stack.is_empty() => return Ok(out),
            Event::Eof => return Err(Error::Protocol("bad XML: unexpected end".into())),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DD_XML: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0" xmlns:av="urn:schemas-sony-com:av">
  <specVersion><major>1</major><minor>0</minor></specVersion>
  <device>
    <deviceType>urn:schemas-upnp-org:device:Basic:1</deviceType>
    <friendlyName>Roy&apos;s ZV-E10</friendlyName>
    <manufacturer>Sony Corporation</manufacturer>
    <modelName>ZV-E10</modelName>
    <UDN>uuid:00000000-0000-1010-8000-0123456789ab</UDN>
    <av:X_DigitalImagingDescURL>http://192.168.122.1:64321/DigitalImagingDesc.xml</av:X_DigitalImagingDescURL>
  </device>
</root>"#;

    #[test]
    fn parses_dd_xml_model_and_friendly_name() {
        let (model, friendly, manufacturer, desc) = parse_dd_xml(DD_XML).unwrap();
        assert_eq!(model, "ZV-E10");
        assert_eq!(friendly, "Roy's ZV-E10");
        assert_eq!(manufacturer, "Sony Corporation");
        assert_eq!(
            desc.as_deref(),
            Some("http://192.168.122.1:64321/DigitalImagingDesc.xml")
        );
    }

    #[test]
    fn dd_xml_without_imaging_desc_has_no_link() {
        let xml = r#"<root><device><friendlyName>Cam</friendlyName>
            <manufacturer>Sony</manufacturer><modelName>ILCE-7M4</modelName></device></root>"#;
        let (model, friendly, _, desc) = parse_dd_xml(xml).unwrap();
        assert_eq!((model.as_str(), friendly.as_str()), ("ILCE-7M4", "Cam"));
        assert_eq!(desc, None);
    }

    #[test]
    fn dd_xml_without_model_is_an_error() {
        let xml = "<root><device><friendlyName>Cam</friendlyName></device></root>";
        assert!(matches!(parse_dd_xml(xml), Err(Error::Protocol(_))));
    }

    #[test]
    fn malformed_xml_is_an_error() {
        assert!(matches!(
            parse_dd_xml("<root><device></root>"),
            Err(Error::Protocol(_))
        ));
    }

    #[test]
    fn parses_digital_imaging_desc_flags() {
        let xml = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-sony-com:av">
  <X_PTP_Information>
    <X_PTP_Versions>3.00</X_PTP_Versions>
    <X_PTP_PairingNecessity>true</X_PTP_PairingNecessity>
    <X_SSH_Support>0</X_SSH_Support>
  </X_PTP_Information>
</root>"#;
        assert_eq!(
            parse_digital_imaging_desc(xml).unwrap(),
            (Some("3.00".into()), Some(true), Some(false))
        );
    }

    #[test]
    fn digital_imaging_desc_fields_are_optional() {
        let xml =
            "<root><X_PTP_Information><X_SSH_Support>1</X_SSH_Support></X_PTP_Information></root>";
        assert_eq!(
            parse_digital_imaging_desc(xml).unwrap(),
            (None, None, Some(true))
        );
    }
}
