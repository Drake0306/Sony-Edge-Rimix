use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::ptp::codes::*;

/// A typed PTP value as carried in device property datasets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PtpValue {
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    Str(String),
    Array(Vec<PtpValue>),
}

fn take<'a>(buf: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], Error> {
    let end = pos
        .checked_add(n)
        .filter(|&e| e <= buf.len())
        .ok_or_else(|| Error::Protocol("truncated PTP value".into()))?;
    let s = &buf[*pos..end];
    *pos = end;
    Ok(s)
}

fn take_arr<const N: usize>(buf: &[u8], pos: &mut usize) -> Result<[u8; N], Error> {
    let mut a = [0u8; N];
    a.copy_from_slice(take(buf, pos, N)?);
    Ok(a)
}

impl PtpValue {
    /// Decode one value of datatype `dt` from `buf` at `*pos`, advancing `*pos`.
    pub fn decode(dt: u16, buf: &[u8], pos: &mut usize) -> Result<PtpValue, Error> {
        if dt & DT_ARRAY_FLAG != 0 && dt != DT_STR {
            let elem = dt & !DT_ARRAY_FLAG;
            let count = u32::from_le_bytes(take_arr(buf, pos)?) as usize;
            // Every element is at least one byte, so a larger count cannot be valid.
            if count > buf.len().saturating_sub(*pos) {
                return Err(Error::Protocol("array count exceeds data".into()));
            }
            let mut items = Vec::with_capacity(count);
            for _ in 0..count {
                items.push(Self::decode(elem, buf, pos)?);
            }
            return Ok(PtpValue::Array(items));
        }
        Ok(match dt {
            DT_INT8 => PtpValue::I8(i8::from_le_bytes(take_arr(buf, pos)?)),
            DT_UINT8 => PtpValue::U8(u8::from_le_bytes(take_arr(buf, pos)?)),
            DT_INT16 => PtpValue::I16(i16::from_le_bytes(take_arr(buf, pos)?)),
            DT_UINT16 => PtpValue::U16(u16::from_le_bytes(take_arr(buf, pos)?)),
            DT_INT32 => PtpValue::I32(i32::from_le_bytes(take_arr(buf, pos)?)),
            DT_UINT32 => PtpValue::U32(u32::from_le_bytes(take_arr(buf, pos)?)),
            DT_INT64 => PtpValue::I64(i64::from_le_bytes(take_arr(buf, pos)?)),
            DT_UINT64 => PtpValue::U64(u64::from_le_bytes(take_arr(buf, pos)?)),
            DT_STR => {
                let count = take(buf, pos, 1)?[0] as usize;
                let raw = take(buf, pos, count * 2)?;
                let units: Vec<u16> = raw
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .take_while(|&u| u != 0)
                    .collect();
                PtpValue::Str(String::from_utf16_lossy(&units))
            }
            _ => return Err(Error::Unsupported("PTP datatype")),
        })
    }

    /// Append the wire encoding of this value to `out`.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            PtpValue::I8(v) => out.extend(v.to_le_bytes()),
            PtpValue::U8(v) => out.extend(v.to_le_bytes()),
            PtpValue::I16(v) => out.extend(v.to_le_bytes()),
            PtpValue::U16(v) => out.extend(v.to_le_bytes()),
            PtpValue::I32(v) => out.extend(v.to_le_bytes()),
            PtpValue::U32(v) => out.extend(v.to_le_bytes()),
            PtpValue::I64(v) => out.extend(v.to_le_bytes()),
            PtpValue::U64(v) => out.extend(v.to_le_bytes()),
            PtpValue::Str(s) => {
                if s.is_empty() {
                    out.push(0);
                } else {
                    let units: Vec<u16> = s.encode_utf16().take(254).collect();
                    // Count includes the terminating NUL and is a single byte on the wire.
                    out.push((units.len() + 1) as u8);
                    for u in &units {
                        out.extend(u.to_le_bytes());
                    }
                    out.extend(0u16.to_le_bytes());
                }
            }
            PtpValue::Array(items) => {
                out.extend((items.len() as u32).to_le_bytes());
                for i in items {
                    i.encode(out);
                }
            }
        }
    }

    /// The value as an unsigned integer, if it is a non-negative scalar integer.
    pub fn as_u64(&self) -> Option<u64> {
        match *self {
            PtpValue::U8(v) => Some(v as u64),
            PtpValue::U16(v) => Some(v as u64),
            PtpValue::U32(v) => Some(v as u64),
            PtpValue::U64(v) => Some(v),
            PtpValue::I8(v) => u64::try_from(v).ok(),
            PtpValue::I16(v) => u64::try_from(v).ok(),
            PtpValue::I32(v) => u64::try_from(v).ok(),
            PtpValue::I64(v) => u64::try_from(v).ok(),
            _ => None,
        }
    }

    /// The value as a signed integer, if it is a scalar integer that fits.
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            PtpValue::I8(v) => Some(v as i64),
            PtpValue::I16(v) => Some(v as i64),
            PtpValue::I32(v) => Some(v as i64),
            PtpValue::I64(v) => Some(v),
            PtpValue::U8(v) => Some(v as i64),
            PtpValue::U16(v) => Some(v as i64),
            PtpValue::U32(v) => Some(v as i64),
            PtpValue::U64(v) => i64::try_from(v).ok(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    fn dec(dt: u16, b: &[u8]) -> PtpValue {
        let mut pos = 0;
        let v = PtpValue::decode(dt, b, &mut pos).unwrap();
        assert_eq!(pos, b.len());
        v
    }

    #[test]
    fn decodes_each_scalar_type() {
        assert_eq!(dec(DT_INT8, &[0xFF]), PtpValue::I8(-1));
        assert_eq!(dec(DT_UINT8, &[0xFF]), PtpValue::U8(255));
        assert_eq!(dec(DT_INT16, &[0xFE, 0xFF]), PtpValue::I16(-2));
        assert_eq!(dec(DT_UINT16, &[0x34, 0x12]), PtpValue::U16(0x1234));
        assert_eq!(dec(DT_INT32, &[0xFE, 0xFF, 0xFF, 0xFF]), PtpValue::I32(-2));
        assert_eq!(dec(DT_UINT32, &[1, 2, 3, 4]), PtpValue::U32(0x0403_0201));
        assert_eq!(dec(DT_INT64, &[0xFF; 8]), PtpValue::I64(-1));
        assert_eq!(
            dec(DT_UINT64, &[1, 0, 0, 0, 0, 0, 0, 0x80]),
            PtpValue::U64(0x8000_0000_0000_0001)
        );
    }

    #[test]
    fn decodes_ptp_string() {
        let b = [3, 0x41, 0, 0x42, 0, 0, 0];
        assert_eq!(dec(DT_STR, &b), PtpValue::Str("AB".into()));
    }

    #[test]
    fn empty_string_is_one_byte() {
        assert_eq!(dec(DT_STR, &[0]), PtpValue::Str(String::new()));
        let mut out = Vec::new();
        PtpValue::Str(String::new()).encode(&mut out);
        assert_eq!(out, [0]);
    }

    #[test]
    fn decodes_array() {
        let b = [2, 0, 0, 0, 1, 0, 2, 0];
        assert_eq!(
            dec(DT_ARRAY_FLAG | DT_UINT16, &b),
            PtpValue::Array(vec![PtpValue::U16(1), PtpValue::U16(2)])
        );
    }

    #[test]
    fn truncated_input_is_protocol_error() {
        let mut pos = 0;
        assert!(matches!(
            PtpValue::decode(DT_UINT32, &[1, 2], &mut pos),
            Err(Error::Protocol(_))
        ));
    }

    #[test]
    fn encode_matches_decode_for_u32_and_str() {
        let mut out = Vec::new();
        PtpValue::U32(0xDEAD_BEEF).encode(&mut out);
        assert_eq!(dec(DT_UINT32, &out), PtpValue::U32(0xDEAD_BEEF));
        let mut out = Vec::new();
        PtpValue::Str("DSC".into()).encode(&mut out);
        assert_eq!(out, [4, b'D', 0, b'S', 0, b'C', 0, 0, 0]);
        assert_eq!(dec(DT_STR, &out), PtpValue::Str("DSC".into()));
    }

    #[test]
    fn unknown_datatype_is_unsupported_error() {
        let mut pos = 0;
        assert!(matches!(
            PtpValue::decode(0x0042, &[0; 8], &mut pos),
            Err(Error::Unsupported(_))
        ));
    }

    #[test]
    fn as_u64_and_as_i64() {
        assert_eq!(PtpValue::U16(7).as_u64(), Some(7));
        assert_eq!(PtpValue::I16(-7).as_u64(), None);
        assert_eq!(PtpValue::I16(-7).as_i64(), Some(-7));
        assert_eq!(PtpValue::Str("x".into()).as_i64(), None);
    }
}
