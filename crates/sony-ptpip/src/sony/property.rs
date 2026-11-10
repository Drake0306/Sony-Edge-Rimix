//! Sony extended device properties: the 0x9209 blob, single reads and writes.

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::ptp::codes::{
    OP_SDIO_GET_ALL_EXT_DEVICE_PROP_INFO, OP_SDIO_GET_EXT_DEVICE_PROP,
    OP_SDIO_SET_EXT_DEVICE_PROP_VALUE,
};
use crate::ptp::PtpValue;
use crate::PtpIpConnection;

/// The values a property may take.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Form {
    None,
    Range {
        min: PtpValue,
        max: PtpValue,
        step: PtpValue,
    },
    Enum {
        settable: Vec<PtpValue>,
        reportable: Vec<PtpValue>,
    },
}

/// Whether the camera currently lets the property be used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Enabled {
    Off,
    On,
    DisplayOnly,
}

/// One entry of the camera's property table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyDesc {
    pub code: u16,
    pub datatype: u16,
    pub writable: bool,
    pub enabled: Enabled,
    pub default: PtpValue,
    pub current: PtpValue,
    pub form: Form,
}

/// A second enum list count is below this; anything else is the next code.
const SECOND_LIST_LIMIT: u16 = 0x0200;

impl PropertyDesc {
    /// True if `value` is one the camera lists as settable for this property:
    /// in the enum's settable list, or inside the range on a step. A property
    /// with no form accepts any value of the same type as its current value.
    pub fn accepts(&self, value: &PtpValue) -> bool {
        if std::mem::discriminant(value) != std::mem::discriminant(&self.current) {
            return false;
        }
        match &self.form {
            Form::None => true,
            Form::Enum { settable, .. } => settable.contains(value),
            Form::Range { min, max, step } => {
                let (Some(v), Some(min), Some(max), Some(step)) =
                    (wide(value), wide(min), wide(max), wide(step))
                else {
                    return false;
                };
                v >= min && v <= max && (step <= 0 || (v - min) % step == 0)
            }
        }
    }
}

/// An integer value widened so every PTP integer type compares correctly.
fn wide(v: &PtpValue) -> Option<i128> {
    v.as_i64()
        .map(i128::from)
        .or_else(|| v.as_u64().map(i128::from))
}

fn read_bytes<'a>(buf: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], Error> {
    let bytes = pos
        .checked_add(n)
        .and_then(|end| buf.get(*pos..end))
        .ok_or_else(|| Error::Protocol("property data truncated".into()))?;
    *pos += n;
    Ok(bytes)
}

fn read_u8(buf: &[u8], pos: &mut usize) -> Result<u8, Error> {
    Ok(read_bytes(buf, pos, 1)?[0])
}

fn read_u16(buf: &[u8], pos: &mut usize) -> Result<u16, Error> {
    let b = read_bytes(buf, pos, 2)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn read_list(dt: u16, buf: &[u8], pos: &mut usize) -> Result<Vec<PtpValue>, Error> {
    let n = read_u16(buf, pos)?;
    (0..n).map(|_| PtpValue::decode(dt, buf, pos)).collect()
}

/// Parses one entry at `*pos`. A datatype `PtpValue` cannot decode yields
/// `Error::Unsupported`, with `*pos` left somewhere inside the entry.
fn parse_entry(buf: &[u8], pos: &mut usize) -> Result<PropertyDesc, Error> {
    let code = read_u16(buf, pos)?;
    let datatype = read_u16(buf, pos)?;
    let writable = read_u8(buf, pos)? != 0;
    let enabled = match read_u8(buf, pos)? {
        0 => Enabled::Off,
        1 => Enabled::On,
        2 => Enabled::DisplayOnly,
        n => {
            return Err(Error::Protocol(format!(
                "property {code:#06x} has enabled flag {n}"
            )))
        }
    };
    let default = PtpValue::decode(datatype, buf, pos)?;
    let current = PtpValue::decode(datatype, buf, pos)?;
    let form = match read_u8(buf, pos)? {
        0 => Form::None,
        1 => Form::Range {
            min: PtpValue::decode(datatype, buf, pos)?,
            max: PtpValue::decode(datatype, buf, pos)?,
            step: PtpValue::decode(datatype, buf, pos)?,
        },
        2 => {
            let settable = read_list(datatype, buf, pos)?;
            // A small u16 here counts a second (reportable) list; a larger
            // one is the next entry's property code and is left in place.
            let mut peek = *pos;
            let reportable = match read_u16(buf, &mut peek) {
                Ok(n) if n < SECOND_LIST_LIMIT => read_list(datatype, buf, pos)?,
                _ => Vec::new(),
            };
            Form::Enum {
                settable,
                reportable,
            }
        }
        n => {
            return Err(Error::Protocol(format!(
                "property {code:#06x} has form flag {n}"
            )))
        }
    };
    Ok(PropertyDesc {
        code,
        datatype,
        writable,
        enabled,
        default,
        current,
        form,
    })
}

/// Parses the 0x9209 data phase: a u64 count followed by that many entries.
///
/// An entry with a datatype that cannot be decoded has no knowable width, so
/// parsing stops there and the entries before it are returned.
pub fn parse_all_props(buf: &[u8]) -> Result<Vec<PropertyDesc>, Error> {
    let mut pos = 0;
    let count = u64::from_le_bytes(
        read_bytes(buf, &mut pos, 8)?
            .try_into()
            .expect("eight bytes"),
    );
    let mut props = Vec::new();
    for _ in 0..count {
        let start = pos;
        match parse_entry(buf, &mut pos) {
            Ok(p) => props.push(p),
            Err(Error::Unsupported(_)) => {
                let code = u16::from_le_bytes([buf[start], buf[start + 1]]);
                let datatype = u16::from_le_bytes([buf[start + 2], buf[start + 3]]);
                tracing::warn!(
                    code = format_args!("{code:#06x}"),
                    datatype = format_args!("{datatype:#06x}"),
                    "unknown property datatype; ignoring the rest of the property list"
                );
                break;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(props)
}

/// Appends one entry in the 0x9209 / 0x9251 layout.
pub fn encode_property(prop: &PropertyDesc, out: &mut Vec<u8>) {
    out.extend(prop.code.to_le_bytes());
    out.extend(prop.datatype.to_le_bytes());
    out.push(prop.writable as u8);
    out.push(match prop.enabled {
        Enabled::Off => 0,
        Enabled::On => 1,
        Enabled::DisplayOnly => 2,
    });
    prop.default.encode(out);
    prop.current.encode(out);
    match &prop.form {
        Form::None => out.push(0),
        Form::Range { min, max, step } => {
            out.push(1);
            for v in [min, max, step] {
                v.encode(out);
            }
        }
        Form::Enum {
            settable,
            reportable,
        } => {
            out.push(2);
            let mut list = |values: &[PtpValue]| {
                out.extend((values.len() as u16).to_le_bytes());
                for v in values {
                    v.encode(out);
                }
            };
            list(settable);
            if !reportable.is_empty() {
                list(reportable);
            }
        }
    }
}

/// Encodes a 0x9209 data phase; the inverse of [`parse_all_props`].
pub fn encode_all_props(props: &[PropertyDesc]) -> Vec<u8> {
    let mut out = (props.len() as u64).to_le_bytes().to_vec();
    for p in props {
        encode_property(p, &mut out);
    }
    out
}

/// Reads every property the camera exposes (0x9209 mode 0).
pub async fn read_all_props(conn: &mut PtpIpConnection) -> Result<Vec<PropertyDesc>, Error> {
    let (data, _) = conn
        .command_data_in(OP_SDIO_GET_ALL_EXT_DEVICE_PROP_INFO, &[0])
        .await?;
    parse_all_props(&data)
}

/// Reads the properties changed since the previous call (0x9209 mode 1).
pub async fn read_changed_props(conn: &mut PtpIpConnection) -> Result<Vec<PropertyDesc>, Error> {
    let (data, _) = conn
        .command_data_in(OP_SDIO_GET_ALL_EXT_DEVICE_PROP_INFO, &[1])
        .await?;
    parse_all_props(&data)
}

/// Reads one property (0x9251).
pub async fn read_property(conn: &mut PtpIpConnection, code: u16) -> Result<PropertyDesc, Error> {
    let (data, _) = conn
        .command_data_in(OP_SDIO_GET_EXT_DEVICE_PROP, &[code as u32])
        .await?;
    let prop = parse_entry(&data, &mut 0)?;
    if prop.code != code {
        return Err(Error::Protocol(format!(
            "asked for property {code:#06x}, got {:#06x}",
            prop.code
        )));
    }
    Ok(prop)
}

/// Writes a property value (0x9205). The caller must only pass values the
/// property's own form allows; see [`PropertyDesc::accepts`].
pub async fn set_property(
    conn: &mut PtpIpConnection,
    code: u16,
    value: &PtpValue,
) -> Result<(), Error> {
    let mut data = Vec::new();
    value.encode(&mut data);
    conn.command_data_out(OP_SDIO_SET_EXT_DEVICE_PROP_VALUE, &[code as u32], &data)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ptp::codes::*;

    fn blob(count: u64, entries: &[&[u8]]) -> Vec<u8> {
        let mut out = count.to_le_bytes().to_vec();
        for e in entries {
            out.extend_from_slice(e);
        }
        out
    }

    /// 0x500E exposure program, u16, writable, enabled, default 1, current 3,
    /// enum [1, 2, 3, 4].
    const EXPOSURE_ENTRY: &[u8] = &[
        0x0E, 0x50, 0x04, 0x00, 1, 1, // code, datatype, writable, enabled
        1, 0, 3, 0, // default, current
        2, 4, 0, 1, 0, 2, 0, 3, 0, 4, 0, // enum, 4 values
    ];

    #[test]
    fn parses_single_enum_entry() {
        let props = parse_all_props(&blob(1, &[EXPOSURE_ENTRY])).unwrap();
        assert_eq!(
            props,
            vec![PropertyDesc {
                code: 0x500E,
                datatype: DT_UINT16,
                writable: true,
                enabled: Enabled::On,
                default: PtpValue::U16(1),
                current: PtpValue::U16(3),
                form: Form::Enum {
                    settable: (1..=4).map(PtpValue::U16).collect(),
                    reportable: vec![],
                },
            }]
        );
    }

    #[test]
    fn parses_range_entry() {
        let entry: &[u8] = &[
            0x0F, 0xD2, 0x04, 0x00, 1, 2, // 0xD20F u16, writable, display only
            0x50, 0x14, 0x50, 0x14, // default, current 5200
            1, 0xC4, 0x09, 0xAC, 0x26, 0x64, 0x00, // range 2500..9900 step 100
        ];
        let props = parse_all_props(&blob(1, &[entry])).unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].enabled, Enabled::DisplayOnly);
        assert_eq!(props[0].current, PtpValue::U16(5200));
        assert_eq!(
            props[0].form,
            Form::Range {
                min: PtpValue::U16(2500),
                max: PtpValue::U16(9900),
                step: PtpValue::U16(100),
            }
        );
    }

    #[test]
    fn parses_second_reportable_list() {
        let first: &[u8] = &[
            0x05, 0x50, 0x04, 0x00, 1, 1, // 0x5005 u16
            2, 0, 2, 0, // default, current
            2, 2, 0, 2, 0, 4, 0, // settable [2, 4]
            3, 0, 2, 0, 4, 0, 6, 0, // reportable [2, 4, 6]
        ];
        let second: &[u8] = &[
            0x1E, 0xD2, 0x06, 0x00, 0, 1, // 0xD21E u32, read-only
            0x20, 0x03, 0, 0, 0x20, 0x03, 0, 0, // 800, 800
            0, // no form
        ];
        let props = parse_all_props(&blob(2, &[first, second])).unwrap();
        assert_eq!(
            props[0].form,
            Form::Enum {
                settable: vec![PtpValue::U16(2), PtpValue::U16(4)],
                reportable: vec![PtpValue::U16(2), PtpValue::U16(4), PtpValue::U16(6)],
            }
        );
        assert_eq!(props[1].code, 0xD21E);
        assert!(!props[1].writable);
        assert_eq!(props[1].current, PtpValue::U32(800));
        assert_eq!(props[1].form, Form::None);
    }

    #[test]
    fn enum_followed_by_next_code_has_no_reportable_list() {
        let props = parse_all_props(&blob(2, &[EXPOSURE_ENTRY, EXPOSURE_ENTRY])).unwrap();
        assert_eq!(props.len(), 2);
        assert!(matches!(&props[0].form, Form::Enum { reportable, .. } if reportable.is_empty()));
    }

    #[test]
    fn skips_entry_with_unknown_datatype_and_keeps_following_entries() {
        let unknown: &[u8] = &[0x99, 0xD2, 0x42, 0x00, 0, 1, 0xAA, 0xBB, 0xCC, 0xDD, 0];
        let props = parse_all_props(&blob(3, &[EXPOSURE_ENTRY, unknown, EXPOSURE_ENTRY])).unwrap();
        // The unknown entry's width cannot be known, so parsing stops there.
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].code, 0x500E);
    }

    #[test]
    fn truncated_blob_is_protocol_error() {
        let b = blob(1, &[&EXPOSURE_ENTRY[..8]]);
        assert!(matches!(parse_all_props(&b), Err(Error::Protocol(_))));
    }

    #[test]
    fn encode_matches_hand_built_bytes() {
        let props = parse_all_props(&blob(1, &[EXPOSURE_ENTRY])).unwrap();
        assert_eq!(encode_all_props(&props), blob(1, &[EXPOSURE_ENTRY]));
    }

    #[test]
    fn accepts_only_listed_or_in_range_values() {
        let props = parse_all_props(&blob(1, &[EXPOSURE_ENTRY])).unwrap();
        assert!(props[0].accepts(&PtpValue::U16(4)));
        assert!(!props[0].accepts(&PtpValue::U16(5)));
        assert!(!props[0].accepts(&PtpValue::U32(4)));
        let mut range = props[0].clone();
        range.form = Form::Range {
            min: PtpValue::U16(2500),
            max: PtpValue::U16(9900),
            step: PtpValue::U16(100),
        };
        assert!(range.accepts(&PtpValue::U16(2500)));
        assert!(range.accepts(&PtpValue::U16(5200)));
        assert!(range.accepts(&PtpValue::U16(9900)));
        assert!(!range.accepts(&PtpValue::U16(5250)));
        assert!(!range.accepts(&PtpValue::U16(10000)));
        assert!(!range.accepts(&PtpValue::U32(5200)));
    }
}
