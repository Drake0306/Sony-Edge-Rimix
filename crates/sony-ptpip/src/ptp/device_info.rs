use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::ptp::codes::*;
use crate::ptp::value::PtpValue;

const STANDARD_VERSION: u16 = 0x0064;
const VENDOR_EXT_VERSION: u16 = 0x0064;
const VENDOR_EXT_DESC: &str = "Sony PTP Extensions";
const MANUFACTURER: &str = "Sony Corporation";

/// The fields of a PTP DeviceInfo dataset that Rimix uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub vendor_ext_id: u32,
    pub model: String,
    pub device_version: String,
    pub operations: Vec<u16>,
}

fn decode(dt: u16, buf: &[u8], pos: &mut usize) -> Result<PtpValue, Error> {
    PtpValue::decode(dt, buf, pos)
}

fn string(buf: &[u8], pos: &mut usize) -> Result<String, Error> {
    match decode(DT_STR, buf, pos)? {
        PtpValue::Str(s) => Ok(s),
        _ => unreachable!("DT_STR decodes to Str"),
    }
}

fn u16_array(buf: &[u8], pos: &mut usize) -> Result<Vec<u16>, Error> {
    match decode(DT_ARRAY_FLAG | DT_UINT16, buf, pos)? {
        PtpValue::Array(items) => Ok(items
            .into_iter()
            .map(|v| match v {
                PtpValue::U16(x) => x,
                _ => unreachable!("UINT16 array holds U16"),
            })
            .collect()),
        _ => unreachable!("array datatype decodes to Array"),
    }
}

fn int(dt: u16, buf: &[u8], pos: &mut usize) -> Result<u64, Error> {
    Ok(decode(dt, buf, pos)?
        .as_u64()
        .expect("unsigned scalar decodes to an integer"))
}

/// Parses a standard PTP DeviceInfo dataset.
pub fn parse_device_info(buf: &[u8]) -> Result<DeviceInfo, Error> {
    let pos = &mut 0;
    int(DT_UINT16, buf, pos)?; // StandardVersion
    let vendor_ext_id = int(DT_UINT32, buf, pos)? as u32;
    int(DT_UINT16, buf, pos)?; // VendorExtensionVersion
    string(buf, pos)?; // VendorExtensionDesc
    int(DT_UINT16, buf, pos)?; // FunctionalMode
    let operations = u16_array(buf, pos)?;
    for _ in 0..4 {
        // Events, DeviceProperties, CaptureFormats, ImageFormats
        u16_array(buf, pos)?;
    }
    string(buf, pos)?; // Manufacturer
    let model = string(buf, pos)?;
    let device_version = string(buf, pos)?;
    string(buf, pos)?; // SerialNumber
    Ok(DeviceInfo {
        vendor_ext_id,
        model,
        device_version,
        operations,
    })
}

fn u16_array_value(items: &[u16]) -> PtpValue {
    PtpValue::Array(items.iter().map(|&x| PtpValue::U16(x)).collect())
}

/// Encodes a standard PTP DeviceInfo dataset; fields Rimix does not model are
/// written as fixed Sony values or empty lists.
pub fn encode_device_info(info: &DeviceInfo) -> Vec<u8> {
    let mut out = Vec::new();
    for v in [
        PtpValue::U16(STANDARD_VERSION),
        PtpValue::U32(info.vendor_ext_id),
        PtpValue::U16(VENDOR_EXT_VERSION),
        PtpValue::Str(VENDOR_EXT_DESC.into()),
        PtpValue::U16(0), // FunctionalMode
        u16_array_value(&info.operations),
        u16_array_value(&[]), // EventsSupported
        u16_array_value(&[]), // DevicePropertiesSupported
        u16_array_value(&[]), // CaptureFormats
        u16_array_value(&[]), // ImageFormats
        PtpValue::Str(MANUFACTURER.into()),
        PtpValue::Str(info.model.clone()),
        PtpValue::Str(info.device_version.clone()),
        PtpValue::Str(String::new()), // SerialNumber
    ] {
        v.encode(&mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DeviceInfo {
        DeviceInfo {
            vendor_ext_id: 0x11,
            model: "ZV-E10".into(),
            device_version: "2.00".into(),
            operations: vec![OP_GET_DEVICE_INFO, OP_SDIO_CONNECT],
        }
    }

    #[test]
    fn round_trips() {
        let info = sample();
        assert_eq!(parse_device_info(&encode_device_info(&info)).unwrap(), info);
    }

    #[test]
    fn starts_with_standard_header() {
        let b = encode_device_info(&sample());
        assert_eq!(&b[..8], &[0x64, 0, 0x11, 0, 0, 0, 0x64, 0]);
    }

    #[test]
    fn truncated_is_protocol_error() {
        let b = encode_device_info(&sample());
        assert!(matches!(
            parse_device_info(&b[..b.len() - 3]),
            Err(Error::Protocol(_))
        ));
    }
}
