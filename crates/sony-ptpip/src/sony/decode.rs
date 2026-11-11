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

/// What a value Rimix has no name for shows as. The code stays visible so
/// a report can name it.
fn unknown(raw: u16) -> String {
    format!("Unknown (0x{raw:04X})")
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

/// White balance values meaning "use the colour temperature": 0x8012 on
/// the bodies libgphoto2 lists, 0x8007 in Rimix's first notes.
pub const WB_COLOUR_TEMP: [u16; 2] = [0x8012, 0x8007];

pub fn wb_label(raw: u16, colour_temp: Option<u16>) -> String {
    match raw {
        0x0001 => "Manual",
        0x0002 => "Auto",
        0x0003 => "One push",
        0x0004 => "Daylight",
        0x0005 => "Fluorescent",
        0x0006 => "Incandescent",
        0x0007 => "Flash",
        0x8001 => "Fluorescent warm white",
        0x8002 => "Fluorescent cool white",
        0x8003 => "Fluorescent day white",
        0x8004 => "Fluorescent daylight",
        0x8010 => "Cloudy",
        0x8011 => "Shade",
        raw if WB_COLOUR_TEMP.contains(&raw) => {
            return match colour_temp {
                Some(k) => format!("{k}K"),
                None => "Colour temp".to_string(),
            }
        }
        0x8020 => "Custom 1",
        0x8021 => "Custom 2",
        0x8022 => "Custom 3",
        0x8023 => "Custom",
        0x8030 => "Underwater auto",
        _ => return unknown(raw),
    }
    .to_string()
}

/// The shooting mode. The names follow the mode dial and Sony's own
/// tables (Camera Remote SDK `CrExposureProgram`, libgphoto2).
pub fn exposure_program_label(raw: u16) -> String {
    match raw {
        0x0001 => "M",
        0x0002 => "P",
        0x0003 => "A",
        0x0004 => "S",
        0x0005 => "Creative",
        0x0006 => "Action",
        0x0007 => "Portrait",
        0x8000 => "Auto",
        0x8001 => "Auto+",
        0x8008 => "P (aperture shift)",
        0x8009 => "P (shutter shift)",
        0x8011 => "Sports",
        0x8012 => "Sunset",
        0x8013 => "Night scene",
        0x8014 => "Landscape",
        0x8015 => "Macro",
        0x8016 => "Handheld twilight",
        0x8017 => "Night portrait",
        0x8018 => "Anti motion blur",
        0x8019 => "Pet",
        0x801A => "Gourmet",
        0x801B => "Fireworks",
        0x801C => "High sensitivity",
        0x8020 => "Memory recall",
        0x8031 => "Continuous priority, 8 fps",
        0x8032 => "Continuous priority, 10 fps",
        0x8033 => "Continuous priority, 12 fps",
        0x8040 => "3D panorama",
        0x8041 => "Panorama",
        0x8050 => "Movie P",
        0x8051 => "Movie A",
        0x8052 => "Movie S",
        0x8053 => "Movie M",
        0x8054 => "Movie Auto",
        0x8055 => "Movie F",
        0x8059 | 0x8084 => "S&Q P",
        0x805A | 0x8085 => "S&Q A",
        0x805B | 0x8086 => "S&Q S",
        0x805C | 0x8087 => "S&Q M",
        0x805D => "S&Q Auto",
        0x805E => "S&Q F",
        0x8060 => "Auto, flash off",
        0x8070 => "Picture effect",
        0x8080 => "High frame rate P",
        0x8081 => "High frame rate A",
        0x8082 => "High frame rate S",
        0x8083 => "High frame rate M",
        0x8088 => "Movie",
        0x8089 => "Photo",
        _ => return unknown(raw),
    }
    .to_string()
}

pub fn focus_mode_label(raw: u16) -> String {
    match raw {
        1 => "MF",
        2 => "AF-S",
        0x8004 => "AF-C",
        0x8005 => "AF-A",
        0x8006 => "DMF",
        0x8009 => "Preset focus",
        _ => return unknown(raw),
    }
    .to_string()
}

/// The drive mode, exposure brackets included: those are `0x8NSS`, with
/// the number of shots in `N` and the step and kind in `SS`.
pub fn drive_label(raw: u16) -> String {
    let named = match raw {
        0x0001 => "Single",
        0x0002 => "Continuous Hi",
        0x8010 => "Continuous Hi+",
        0x8015 => "Continuous Mid",
        0x8012 => "Continuous Lo",
        0x8013 => "Continuous",
        0x8014 => "Continuous, speed priority",
        0x8003 => "Self-timer 5 s",
        0x8004 => "Self-timer 10 s",
        0x8005 => "Self-timer 2 s",
        0x8008 => "Self-timer 10 s, 3 shots",
        0x8009 => "Self-timer 10 s, 5 shots",
        0x800C => "Self-timer 5 s, 3 shots",
        0x800D => "Self-timer 5 s, 5 shots",
        0x800E => "Self-timer 2 s, 3 shots",
        0x800F => "Self-timer 2 s, 5 shots",
        0x8018 => "White balance bracket Lo",
        0x8028 => "White balance bracket Hi",
        0x8019 => "DRO bracket Lo",
        0x8029 => "DRO bracket Hi",
        _ => "",
    };
    if !named.is_empty() {
        return named.to_string();
    }
    let shots = (raw >> 8) & 0xF;
    let step = match raw & 0xF0 {
        0x30 if raw & 0xF >= 6 => Some("0.3"),
        0x50 => Some("0.5"),
        0x70 => Some("0.7"),
        0x10 => Some("1.0"),
        0x20 => Some("2.0"),
        0x30 => Some("3.0"),
        _ => None,
    };
    let kind = match raw & 0xF {
        7 | 1 => Some("continuous"),
        6 | 0 => Some("single"),
        _ => None,
    };
    match (raw >> 12, shots, step, kind) {
        (8, 3 | 5 | 7 | 9, Some(step), Some(kind)) => {
            format!("Bracket {step} EV, {shots} shots, {kind}")
        }
        _ => unknown(raw),
    }
}

pub fn format_label(raw: u8) -> String {
    match raw {
        0x01 => "JPEG Light",
        0x02 => "JPEG Standard",
        0x03 => "JPEG Fine",
        0x04 => "JPEG Extra fine",
        0x10 => "RAW",
        0x11 => "RAW+JPEG Light",
        0x12 => "RAW+JPEG Standard",
        0x13 => "RAW+JPEG",
        0x14 => "RAW+JPEG Extra fine",
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
        assert_eq!(wb_label(0x1234, None), "Unknown (0x1234)");
        // Both codes seen for "use the colour temperature".
        assert_eq!(wb_label(0x8012, Some(4300)), "4300K");
        assert_eq!(wb_label(0x8011, None), "Shade");
        assert_eq!(wb_label(0x8010, None), "Cloudy");
        assert_eq!(wb_label(6, None), "Incandescent");
        assert_eq!(wb_label(0x8003, None), "Fluorescent day white");
        assert_eq!(wb_label(7, None), "Flash");
        assert_eq!(wb_label(0x8021, None), "Custom 2");
        assert_eq!(wb_label(0x8030, None), "Underwater auto");
    }

    #[test]
    fn every_mode_a_body_reports_has_a_name() {
        for (raw, name) in [
            (0x8000, "Auto"),
            (0x8001, "Auto+"),
            (0x0007, "Portrait"),
            (0x8011, "Sports"),
            (0x8013, "Night scene"),
            (0x8016, "Handheld twilight"),
            (0x8020, "Memory recall"),
            (0x8041, "Panorama"),
            (0x8054, "Movie Auto"),
            (0x8059, "S&Q P"),
            (0x805C, "S&Q M"),
            (0x805D, "S&Q Auto"),
            (0x8080, "High frame rate P"),
            (0x8084, "S&Q P"),
            (0x8088, "Movie"),
            (0x8089, "Photo"),
        ] {
            assert_eq!(exposure_program_label(raw), name, "{raw:#06x}");
        }
        // No name is a bare number.
        for raw in [1u16, 2, 3, 4, 0x8000, 0x8050, 0x8059] {
            let label = exposure_program_label(raw);
            assert!(
                !label.starts_with("0x") && !label.contains("Unknown"),
                "{label}"
            );
        }
    }

    #[test]
    fn drive_modes_and_brackets_have_names() {
        assert_eq!(drive_label(0x8010), "Continuous Hi+");
        assert_eq!(drive_label(0x8015), "Continuous Mid");
        assert_eq!(drive_label(0x8003), "Self-timer 5 s");
        assert_eq!(drive_label(0x8008), "Self-timer 10 s, 3 shots");
        assert_eq!(drive_label(0x800F), "Self-timer 2 s, 5 shots");
        assert_eq!(drive_label(0x8337), "Bracket 0.3 EV, 3 shots, continuous");
        assert_eq!(drive_label(0x8556), "Bracket 0.5 EV, 5 shots, single");
        assert_eq!(drive_label(0x8911), "Bracket 1.0 EV, 9 shots, continuous");
        assert_eq!(drive_label(0x8320), "Bracket 2.0 EV, 3 shots, single");
        assert_eq!(drive_label(0x8018), "White balance bracket Lo");
        assert_eq!(drive_label(0x8029), "DRO bracket Hi");
        assert_eq!(drive_label(0x7777), "Unknown (0x7777)");
        assert_eq!(format_label(0x14), "RAW+JPEG Extra fine");
        assert_eq!(format_label(0x77), "Unknown (0x0077)");
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
        assert_eq!(exposure_program_label(0x7777), "Unknown (0x7777)");
        assert_eq!(focus_mode_label(1), "MF");
        assert_eq!(focus_mode_label(2), "AF-S");
        assert_eq!(focus_mode_label(0x8004), "AF-C");
        assert_eq!(focus_mode_label(0x8005), "AF-A");
        assert_eq!(focus_mode_label(0x8006), "DMF");
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
