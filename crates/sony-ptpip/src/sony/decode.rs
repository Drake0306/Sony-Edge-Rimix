//! Turn raw property values into labels for the UI and CLI.

use crate::ptp::codes::*;
use crate::ptp::value::PtpValue;
use serde::{Deserialize, Serialize};

/// A display form of a property value: full text and a compact chip form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Labeled {
    pub label: String,
    pub short: String,
}

impl Labeled {
    fn same(s: String) -> Self {
        Labeled {
            short: s.clone(),
            label: s,
        }
    }
}

fn unknown(raw: u16) -> String {
    format!("0x{raw:04X}")
}

/// Label a property value, or `None` when the code is unknown or the value
/// has the wrong type for it.
pub fn label_for(code: u16, value: &PtpValue) -> Option<Labeled> {
    let u16v = || value.as_u64().and_then(|v| u16::try_from(v).ok());
    match code {
        PROP_SHUTTER_SPEED => {
            let raw = u32::try_from(value.as_u64()?).ok()?;
            Some(Labeled::same(shutter_label(raw)))
        }
        PROP_F_NUMBER => Some(Labeled::same(aperture_label(u16v()?))),
        PROP_ISO => {
            let raw = u32::try_from(value.as_u64()?).ok()?;
            let label = iso_label(raw);
            let short = label.strip_prefix("ISO ").unwrap_or(&label).to_string();
            Some(Labeled { label, short })
        }
        PROP_EXPOSURE_BIAS => {
            let raw = i16::try_from(value.as_i64()?).ok()?;
            Some(Labeled::same(ev_label(raw)))
        }
        PROP_WHITE_BALANCE => Some(Labeled::same(wb_label(u16v()?, None))),
        PROP_EXPOSURE_PROGRAM => Some(Labeled::same(exposure_program_label(u16v()?))),
        PROP_FOCUS_MODE => Some(Labeled::same(focus_mode_label(u16v()?))),
        PROP_STILL_CAPTURE_MODE => Some(Labeled::same(drive_label(u16v()?))),
        PROP_COMPRESSION => {
            let raw = u8::try_from(value.as_u64()?).ok()?;
            Some(Labeled::same(format_label(raw)))
        }
        PROP_BATTERY => {
            let raw = u8::try_from(value.as_u64()?).ok()?;
            Some(Labeled::same(battery_label(raw)))
        }
        _ => None,
    }
}

/// Shutter speed from `numerator << 16 | denominator`.
pub fn shutter_label(raw: u32) -> String {
    let num = raw >> 16;
    let den = raw & 0xFFFF;
    match (num, den) {
        (0, _) | (_, 0) => "Bulb".to_string(),
        (n, 1) => format!("{n}\""),
        (1, d) => format!("1/{d}"),
        (n, d) if (n * 10) % d == 0 => format!("{}.{}\"", n / d, (n * 10 / d) % 10),
        (n, d) => format!("{n}/{d}"),
    }
}

/// F-number from hundredths.
pub fn aperture_label(raw: u16) -> String {
    if raw >= 1000 && raw.is_multiple_of(100) {
        format!("f/{}", raw / 100)
    } else {
        format!("f/{}.{}", raw / 100, (raw % 100) / 10)
    }
}

/// ISO from the lower 24 bits; upper byte 0x10 means multi-frame noise reduction.
pub fn iso_label(raw: u32) -> String {
    let value = raw & 0x00FF_FFFF;
    let nr = if raw >> 24 == 0x10 { " NR" } else { "" };
    if value == 0x00FF_FFFF {
        format!("ISO Auto{nr}")
    } else {
        format!("ISO {value}{nr}")
    }
}

/// Exposure compensation from thousandths of an EV.
pub fn ev_label(raw: i16) -> String {
    if raw == 0 {
        return "0.0".to_string();
    }
    let sign = if raw < 0 { '-' } else { '+' };
    let abs = raw.unsigned_abs();
    format!("{sign}{}.{}", abs / 1000, (abs % 1000) / 100)
}

pub fn wb_label(raw: u16, colour_temp: Option<u16>) -> String {
    match raw {
        0x0002 => "Auto".to_string(),
        0x0004 => "Daylight".to_string(),
        0x8007 => match colour_temp {
            Some(k) => format!("{k}K"),
            None => "Colour temp".to_string(),
        },
        _ => unknown(raw),
    }
}

pub fn exposure_program_label(raw: u16) -> String {
    match raw {
        1 => "M",
        2 => "P",
        3 => "A",
        4 => "S",
        0x8050 => "Movie P",
        0x8051 => "Movie A",
        0x8052 => "Movie S",
        0x8053 => "Movie M",
        _ => return unknown(raw),
    }
    .to_string()
}

pub fn focus_mode_label(raw: u16) -> String {
    match raw {
        1 => "MF",
        2 => "AF-S",
        0x8004 => "AF-C",
        0x8005 => "DMF",
        0x8006 => "AF-A",
        _ => return unknown(raw),
    }
    .to_string()
}

pub fn drive_label(raw: u16) -> String {
    match raw {
        1 => "Single",
        2 => "Continuous Hi",
        0x8012 => "Continuous Lo",
        0x8004 => "Self-timer 10 s",
        0x8005 => "Self-timer 2 s",
        _ => return unknown(raw),
    }
    .to_string()
}

pub fn format_label(raw: u8) -> String {
    match raw {
        2 => "JPEG Standard",
        3 => "JPEG Fine",
        4 => "JPEG Extra fine",
        0x10 => "RAW",
        0x13 => "RAW+JPEG",
        0x12 => "RAW+JPEG Standard",
        _ => return unknown(raw.into()),
    }
    .to_string()
}

pub fn battery_label(raw: u8) -> String {
    format!("{raw}%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutter() {
        assert_eq!(shutter_label(0x0001_007D), "1/125");
        assert_eq!(shutter_label(0x000A_0001), "10\"");
        assert_eq!(shutter_label(0), "Bulb");
        assert_eq!(shutter_label(0x0001_0003), "1/3");
        assert_eq!(shutter_label(0x0003_0002), "1.5\"");
        assert_eq!(shutter_label(0x0004_0003), "4/3");
    }

    #[test]
    fn aperture() {
        assert_eq!(aperture_label(400), "f/4.0");
        assert_eq!(aperture_label(1000), "f/10");
        assert_eq!(aperture_label(1100), "f/11");
        assert_eq!(aperture_label(1600), "f/16");
        assert_eq!(aperture_label(560), "f/5.6");
        assert_eq!(aperture_label(630), "f/6.3");
    }

    #[test]
    fn iso() {
        assert_eq!(iso_label(0x00FF_FFFF), "ISO Auto");
        assert_eq!(iso_label(800), "ISO 800");
        assert_eq!(iso_label(0x1000_0320), "ISO 800 NR");
        assert_eq!(iso_label(0x2000_0320), "ISO 800");
    }

    #[test]
    fn ev() {
        assert_eq!(ev_label(300), "+0.3");
        assert_eq!(ev_label(-700), "-0.7");
        assert_eq!(ev_label(0), "0.0");
    }

    #[test]
    fn white_balance() {
        assert_eq!(wb_label(2, None), "Auto");
        assert_eq!(wb_label(4, None), "Daylight");
        assert_eq!(wb_label(0x8007, Some(5200)), "5200K");
        assert_eq!(wb_label(0x8007, None), "Colour temp");
        assert_eq!(wb_label(0x1234, None), "0x1234");
    }

    #[test]
    fn enums() {
        assert_eq!(exposure_program_label(1), "M");
        assert_eq!(exposure_program_label(2), "P");
        assert_eq!(exposure_program_label(3), "A");
        assert_eq!(exposure_program_label(4), "S");
        assert_eq!(exposure_program_label(0x8050), "Movie P");
        assert_eq!(exposure_program_label(0x8051), "Movie A");
        assert_eq!(exposure_program_label(0x8052), "Movie S");
        assert_eq!(exposure_program_label(0x8053), "Movie M");
        assert_eq!(exposure_program_label(0x7777), "0x7777");
        assert_eq!(focus_mode_label(1), "MF");
        assert_eq!(focus_mode_label(2), "AF-S");
        assert_eq!(focus_mode_label(0x8004), "AF-C");
        assert_eq!(focus_mode_label(0x8005), "DMF");
        assert_eq!(focus_mode_label(0x8006), "AF-A");
        assert_eq!(drive_label(1), "Single");
        assert_eq!(drive_label(2), "Continuous Hi");
        assert_eq!(drive_label(0x8012), "Continuous Lo");
        assert_eq!(drive_label(0x8004), "Self-timer 10 s");
        assert_eq!(drive_label(0x8005), "Self-timer 2 s");
        assert_eq!(format_label(2), "JPEG Standard");
        assert_eq!(format_label(3), "JPEG Fine");
        assert_eq!(format_label(4), "JPEG Extra fine");
        assert_eq!(format_label(0x10), "RAW");
        assert_eq!(format_label(0x13), "RAW+JPEG");
        assert_eq!(format_label(0x12), "RAW+JPEG Standard");
        assert_eq!(battery_label(72), "72%");
    }

    #[test]
    fn dispatch() {
        let iso = label_for(PROP_ISO, &PtpValue::U32(800)).unwrap();
        assert_eq!((iso.label.as_str(), iso.short.as_str()), ("ISO 800", "800"));
        let ev = label_for(PROP_EXPOSURE_BIAS, &PtpValue::I16(300)).unwrap();
        assert_eq!(ev.short, "+0.3");
        assert_eq!(
            label_for(PROP_BATTERY, &PtpValue::U8(72)).unwrap().short,
            "72%"
        );
        assert_eq!(
            label_for(PROP_WHITE_BALANCE, &PtpValue::U16(0x8007))
                .unwrap()
                .label,
            "Colour temp"
        );
        assert_eq!(label_for(PROP_ISO, &PtpValue::Str("x".into())), None);
        assert_eq!(label_for(0x1234, &PtpValue::U8(1)), None);
    }
}
