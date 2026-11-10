use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::ptp::codes::*;
use crate::ptp::value::PtpValue;

/// The fields of a PTP ObjectInfo dataset that Rimix uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectInfo {
    pub storage_id: u32,
    pub object_format: u16,
    pub compressed_size: u32,
    pub filename: String,
    pub capture_date: String,
}

fn string(buf: &[u8], pos: &mut usize) -> Result<String, Error> {
    match PtpValue::decode(DT_STR, buf, pos)? {
        PtpValue::Str(s) => Ok(s),
        _ => unreachable!("DT_STR decodes to Str"),
    }
}

fn num(buf: &[u8], pos: &mut usize, dt: u16) -> Result<u64, Error> {
    PtpValue::decode(dt, buf, pos)?
        .as_u64()
        .ok_or_else(|| Error::Protocol("bad ObjectInfo field".into()))
}

/// Parse a standard PTP ObjectInfo dataset (little-endian).
pub fn parse_object_info(buf: &[u8]) -> Result<ObjectInfo, Error> {
    let pos = &mut 0;
    let storage_id = num(buf, pos, DT_UINT32)? as u32;
    let object_format = num(buf, pos, DT_UINT16)? as u16;
    num(buf, pos, DT_UINT16)?; // ProtectionStatus
    let compressed_size = num(buf, pos, DT_UINT32)? as u32;
    num(buf, pos, DT_UINT16)?; // ThumbFormat
    for _ in 0..7 {
        // ThumbCompressedSize, ThumbPixWidth/Height, ImagePixWidth/Height,
        // ImageBitDepth, ParentObject
        num(buf, pos, DT_UINT32)?;
    }
    num(buf, pos, DT_UINT16)?; // AssociationType
    num(buf, pos, DT_UINT32)?; // AssociationDesc
    num(buf, pos, DT_UINT32)?; // SequenceNumber
    let filename = string(buf, pos)?;
    let capture_date = string(buf, pos)?;
    // ModificationDate and Keywords are not needed.
    Ok(ObjectInfo {
        storage_id,
        object_format,
        compressed_size,
        filename,
        capture_date,
    })
}

/// Encode an ObjectInfo dataset; the fields `ObjectInfo` does not carry are
/// written as zeros and empty strings.
pub fn encode_object_info(info: &ObjectInfo) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(info.storage_id.to_le_bytes());
    b.extend(info.object_format.to_le_bytes());
    b.extend(0u16.to_le_bytes()); // ProtectionStatus
    b.extend(info.compressed_size.to_le_bytes());
    b.extend(0u16.to_le_bytes()); // ThumbFormat
    for _ in 0..7 {
        b.extend(0u32.to_le_bytes());
    }
    b.extend(0u16.to_le_bytes()); // AssociationType
    b.extend(0u32.to_le_bytes()); // AssociationDesc
    b.extend(0u32.to_le_bytes()); // SequenceNumber
    PtpValue::Str(info.filename.clone()).encode(&mut b);
    PtpValue::Str(info.capture_date.clone()).encode(&mut b);
    PtpValue::Str(String::new()).encode(&mut b); // ModificationDate
    PtpValue::Str(String::new()).encode(&mut b); // Keywords
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ptp::value::PtpValue;

    #[test]
    fn parses_object_info_filename() {
        let mut b = Vec::new();
        b.extend(0x0001_0001u32.to_le_bytes()); // storage
        b.extend(0x3801u16.to_le_bytes()); // format
        b.extend(0u16.to_le_bytes()); // protection
        b.extend(1234u32.to_le_bytes()); // size
        b.extend(0x3808u16.to_le_bytes()); // thumb format
        for _ in 0..7 {
            b.extend(0u32.to_le_bytes()); // thumb size/w/h, image w/h/depth, parent
        }
        b.extend(0u16.to_le_bytes()); // assoc type
        b.extend(0u32.to_le_bytes()); // assoc desc
        b.extend(0u32.to_le_bytes()); // sequence
        PtpValue::Str("DSC01236.JPG".into()).encode(&mut b);
        PtpValue::Str("20261110T101500".into()).encode(&mut b);
        PtpValue::Str(String::new()).encode(&mut b);
        PtpValue::Str(String::new()).encode(&mut b);
        let oi = parse_object_info(&b).unwrap();
        assert_eq!(oi.storage_id, 0x0001_0001);
        assert_eq!(oi.object_format, 0x3801);
        assert_eq!(oi.compressed_size, 1234);
        assert_eq!(oi.filename, "DSC01236.JPG");
        assert_eq!(oi.capture_date, "20261110T101500");
    }

    #[test]
    fn encoded_object_info_parses_back() {
        let info = ObjectInfo {
            storage_id: 0,
            object_format: 0x3801,
            compressed_size: 0,
            filename: "LiveView.jpg".into(),
            capture_date: String::new(),
        };
        assert_eq!(parse_object_info(&encode_object_info(&info)).unwrap(), info);
    }
}
