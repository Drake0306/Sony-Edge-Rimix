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

/// The lower 16 bits of a value. Bodies from about 2020 on send the
/// shooting mode and the drive mode as 32 bits: the code older bodies send
/// in 16 bits, with a family number above it (`0x0001_0002` is P, where an
/// older body says `0x0002`; `0x0007_8050` is Movie P). libgphoto2 lists
/// them as `sony_exposure_program_32_modes` and `sony3_capturemode`. The
/// lower half alone names the value on both kinds of body.
fn low_word(value: &PtpValue) -> Option<u16> {
    value.as_u64().map(|v| (v & 0xFFFF) as u16)
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
        PROP_EXPOSURE_PROGRAM => {
            let raw = low_word(value)?;
            Some(Labeled {
                label: exposure_program_name(raw),
                short: exposure_program_label(raw),
            })
        }
        PROP_FOCUS_MODE => Some(Labeled::same(focus_mode_label(u16v()?))),
        PROP_STILL_CAPTURE_MODE => {
            let raw = low_word(value)?;
            Some(Labeled {
                label: drive_label(raw),
                short: drive_short(raw),
            })
        }
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

/// Which kind of shooting a mode belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModeFamily {
    Photo,
    Movie,
    SlowQuick,
    HighFrameRate,
}

/// The exposure part of a mode: the letter on the dial, and Sony's name
/// for it in the camera's menus and Help Guide.
fn exposure_kind(raw: u16) -> Option<(ModeFamily, &'static str, &'static str)> {
    use ModeFamily::*;
    const P: (&str, &str) = ("P", "Program Auto");
    const A: (&str, &str) = ("A", "Aperture Priority");
    const S: (&str, &str) = ("S", "Shutter Priority");
    const M: (&str, &str) = ("M", "Manual Exposure");
    const AUTO: (&str, &str) = ("Auto", "Intelligent Auto");
    const F: (&str, &str) = ("F", "Flexible Exposure");
    let (family, (letter, name)) = match raw {
        0x0001 => (Photo, M),
        0x0002 => (Photo, P),
        0x0003 => (Photo, A),
        0x0004 => (Photo, S),
        0x8050 => (Movie, P),
        0x8051 => (Movie, A),
        0x8052 => (Movie, S),
        0x8053 => (Movie, M),
        0x8054 => (Movie, AUTO),
        0x8055 | 0x8090 => (Movie, F),
        0x8059 | 0x8084 => (SlowQuick, P),
        0x805A | 0x8085 => (SlowQuick, A),
        0x805B | 0x8086 => (SlowQuick, S),
        0x805C | 0x8087 => (SlowQuick, M),
        0x805D => (SlowQuick, AUTO),
        0x805E | 0x8091 => (SlowQuick, F),
        0x8080 => (HighFrameRate, P),
        0x8081 => (HighFrameRate, A),
        0x8082 => (HighFrameRate, S),
        0x8083 => (HighFrameRate, M),
        _ => return None,
    };
    Some((family, letter, name))
}

/// The modes that are not an exposure letter: short form, then Sony's name.
fn other_mode(raw: u16) -> Option<(&'static str, &'static str)> {
    Some(match raw {
        0x0005 => ("Creative", "Creative"),
        0x0006 => ("Action", "Action"),
        0x0007 => ("Portrait", "Portrait"),
        0x8000 => ("Auto", "Intelligent Auto"),
        0x8001 => ("Auto+", "Superior Auto"),
        0x8008 => ("P shift A", "Program Auto, aperture shift"),
        0x8009 => ("P shift S", "Program Auto, shutter shift"),
        0x8011 => ("Sports", "Sports Action"),
        0x8012 => ("Sunset", "Sunset"),
        0x8013 => ("Night scene", "Night Scene"),
        0x8014 => ("Landscape", "Landscape"),
        0x8015 => ("Macro", "Macro"),
        0x8016 => ("Twilight", "Hand-held Twilight"),
        0x8017 => ("Night portrait", "Night Portrait"),
        0x8018 => ("Anti blur", "Anti Motion Blur"),
        0x8019 => ("Pet", "Pet"),
        0x801A => ("Gourmet", "Gourmet"),
        0x801B => ("Fireworks", "Fireworks"),
        0x801C => ("High ISO", "High Sensitivity"),
        0x8020 => ("Recall", "Recall Camera Setting"),
        0x8030 => ("Cont. AE", "Continuous Priority AE"),
        0x8031 => ("Cont. AE 8", "Continuous Priority AE, 8 fps"),
        0x8032 => ("Cont. AE 10", "Continuous Priority AE, 10 fps"),
        0x8033 => ("Cont. AE 12", "Continuous Priority AE, 12 fps"),
        0x8040 => ("3D panorama", "3D Sweep Panorama"),
        0x8041 => ("Panorama", "Sweep Panorama"),
        0x8060 => ("Auto, no flash", "Intelligent Auto, flash off"),
        0x8070 => ("Effect", "Picture Effect"),
        0x8088 => ("Movie", "Movie"),
        0x8089 => ("Photo", "Photo"),
        0x808A => ("F", "Flexible Exposure"),
        _ => return None,
    })
}

/// The shooting mode in a few characters, for a chip: "P", "Movie M",
/// "S&Q A", "Auto", "Portrait". `raw` is the lower 16 bits of the value.
pub fn exposure_program_label(raw: u16) -> String {
    if let Some((family, letter, _)) = exposure_kind(raw) {
        return match family {
            ModeFamily::Photo => letter.to_string(),
            ModeFamily::Movie => format!("Movie {letter}"),
            ModeFamily::SlowQuick => format!("S&Q {letter}"),
            ModeFamily::HighFrameRate => format!("HFR {letter}"),
        };
    }
    match other_mode(raw) {
        Some((short, _)) => short.to_string(),
        None => unknown(raw),
    }
}

/// The shooting mode by Sony's own name, with its kind of shooting after
/// it when that is not photos: "Program Auto", "Program Auto (Movie)",
/// "Manual Exposure (S&Q)", "Hand-held Twilight".
pub fn exposure_program_name(raw: u16) -> String {
    if let Some((family, _, name)) = exposure_kind(raw) {
        return match family {
            ModeFamily::Photo => name.to_string(),
            ModeFamily::Movie => format!("{name} (Movie)"),
            ModeFamily::SlowQuick => format!("{name} (S&Q)"),
            ModeFamily::HighFrameRate => format!("{name} (High frame rate)"),
        };
    }
    match other_mode(raw) {
        Some((_, name)) => name.to_string(),
        None => unknown(raw),
    }
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

/// The drive mode, exposure brackets included. `raw` is the lower 16 bits
/// of the value (see [`low_word`]); 0 is what a body reports while the
/// drive mode is switched off, as in movie modes.
///
/// A bracket is `0xNCSK`: `N` is 8 for the usual brackets and C for the
/// two-shot ones, `C` the number of shots, and `SK` the step with, in its
/// last digit, whether the shots are taken in one burst or one by one.
pub fn drive_label(raw: u16) -> String {
    let named = match raw {
        0x0000 => "—",
        0x0001 => "Single",
        0x0002 => "Continuous Hi",
        0x0003 => "Timelapse",
        0x8010 => "Continuous Hi+",
        0x8011 => "Continuous Hi, live",
        0x8012 => "Continuous Lo",
        0x8013 => "Continuous",
        0x8014 => "Continuous, speed priority",
        0x8015 => "Continuous Mid",
        0x8016 => "Continuous Mid, live",
        0x8017 => "Continuous Lo, live",
        0x8003 => "Self-timer 5 s",
        0x8004 => "Self-timer 10 s",
        0x8005 => "Self-timer 2 s",
        0x8006 => "Self-portrait, 1 person",
        0x8007 => "Self-portrait, 2 people",
        0x8008 => "Self-timer 10 s, 3 shots",
        0x8009 => "Self-timer 10 s, 5 shots",
        0x800A => "Remote commander",
        0x800B => "Mirror up",
        0x800C => "Self-timer 5 s, 3 shots",
        0x800D => "Self-timer 5 s, 5 shots",
        0x800E => "Self-timer 2 s, 3 shots",
        0x800F => "Self-timer 2 s, 5 shots",
        0x8018 => "White balance bracket Lo",
        0x8028 => "White balance bracket Hi",
        0x8019 => "DRO bracket Lo",
        0x8029 => "DRO bracket Hi",
        0x801A => "LPF bracket",
        0x8030 => "Spot burst Lo",
        0x8031 => "Spot burst Mid",
        0x8032 => "Spot burst Hi",
        0x8040 => "Focus bracket",
        _ => "",
    };
    if !named.is_empty() {
        return named.to_string();
    }
    bracket_label(raw).unwrap_or_else(|| unknown(raw))
}

/// An exposure bracket's name from its code, or `None` for anything else.
fn bracket_label(raw: u16) -> Option<String> {
    let two_shot = match raw >> 12 {
        0x8 => false,
        0xC => true,
        _ => return None,
    };
    let shots = (raw >> 8) & 0xF;
    let (step_digit, last) = ((raw >> 4) & 0xF, raw & 0xF);
    // Thirds end in 7 (burst) or 6 (one by one); the other steps in 1 or 0.
    // A two-shot bracket going under adds 8 to that last digit.
    let (thirds, burst, minus) = match last {
        0x7 => (true, true, false),
        0x6 => (true, false, false),
        0xF => (true, true, true),
        0xE => (true, false, true),
        0x1 => (false, true, false),
        0x0 => (false, false, false),
        0x9 => (false, true, true),
        0x8 => (false, false, true),
        _ => return None,
    };
    let step = match (thirds, step_digit) {
        (true, 3) => "0.3",
        (true, 5) => "0.5",
        (true, 7) => "0.7",
        (false, 1) => "1.0",
        (false, 4) => "1.3",
        (false, 6) => "1.5",
        (false, 8) => "1.7",
        (false, 2) => "2.0",
        (false, 5) => "2.3",
        (false, 7) => "2.5",
        (false, 9) => "2.7",
        (false, 3) => "3.0",
        _ => return None,
    };
    let kind = if burst { "continuous" } else { "single" };
    match (two_shot, shots, minus) {
        (false, 3 | 5 | 7 | 9, false) => Some(format!("Bracket {step} EV, {shots} shots, {kind}")),
        (true, 2, false) => Some(format!("Bracket {step} EV, 2 shots plus, {kind}")),
        (true, 2, true) => Some(format!("Bracket {step} EV, 2 shots minus, {kind}")),
        _ => None,
    }
}

/// The drive mode in a dozen characters, for a chip: "Cont. Hi+",
/// "Timer 5 s ×3", "BRK C 0.3 ×3" (C for a burst, S for one by one).
pub fn drive_short(raw: u16) -> String {
    let full = drive_label(raw);
    // "Bracket 0.3 EV, 3 shots, continuous" becomes "BRK C 0.3 ×3".
    if let Some(rest) = full.strip_prefix("Bracket ") {
        let kind = if rest.ends_with("continuous") {
            'C'
        } else {
            'S'
        };
        let mut parts = rest.split(", ");
        let step = parts.next().unwrap_or("").trim_end_matches(" EV");
        let shots = parts
            .next()
            .unwrap_or("")
            .replace(" shots plus", "+")
            .replace(" shots minus", "-")
            .replace(" shots", "");
        return format!("BRK {kind} {step} ×{shots}");
    }
    full.replace("Continuous, speed priority", "Cont. speed")
        .replace("Continuous", "Cont.")
        .replace("Self-timer", "Timer")
        .replace("Self-portrait", "Selfie")
        .replace("White balance bracket", "WB BRK")
        .replace("DRO bracket", "DRO BRK")
        .replace("LPF bracket", "LPF BRK")
        .replace("Focus bracket", "Focus BRK")
        .replace("Remote commander", "Remote")
        .replace(", live", " live")
        .replace(", 1 person", " ×1")
        .replace(", 2 people", " ×2")
        .replace(", 3 shots", " ×3")
        .replace(", 5 shots", " ×5")
}

/// White balance in a dozen characters, for a chip.
pub fn wb_short(raw: u16) -> String {
    match raw {
        0x8001 => "Fluor. warm",
        0x8002 => "Fluor. cool",
        0x8003 => "Fluor. day",
        0x8004 => "Fluor. light",
        0x8030 => "Underwater",
        _ => return wb_label(raw, None),
    }
    .to_string()
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
            (0x8016, "Twilight"),
            (0x8020, "Recall"),
            (0x8041, "Panorama"),
            (0x8054, "Movie Auto"),
            (0x8059, "S&Q P"),
            (0x805C, "S&Q M"),
            (0x805D, "S&Q Auto"),
            (0x8080, "HFR P"),
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

    /// Every mode the real ZV-E10 (firmware 2.01) lists, as it sends them:
    /// 32 bits, a family in the upper half and the classic code in the lower.
    #[test]
    fn the_zv_e10s_own_mode_values_all_have_names() {
        let modes: [(u32, &str, &str); 25] = [
            (0x0001_0002, "P", "Program Auto"),
            (0x0002_0003, "A", "Aperture Priority"),
            (0x0003_0004, "S", "Shutter Priority"),
            (0x0000_0001, "M", "Manual Exposure"),
            (0x0007_8050, "Movie P", "Program Auto (Movie)"),
            (0x0007_8051, "Movie A", "Aperture Priority (Movie)"),
            (0x0007_8052, "Movie S", "Shutter Priority (Movie)"),
            (0x0007_8053, "Movie M", "Manual Exposure (Movie)"),
            (0x0007_8054, "Movie Auto", "Intelligent Auto (Movie)"),
            (0x0009_8059, "S&Q P", "Program Auto (S&Q)"),
            (0x0009_805A, "S&Q A", "Aperture Priority (S&Q)"),
            (0x0009_805B, "S&Q S", "Shutter Priority (S&Q)"),
            (0x0009_805C, "S&Q M", "Manual Exposure (S&Q)"),
            (0x0004_8000, "Auto", "Intelligent Auto"),
            (0x0004_8001, "Auto+", "Superior Auto"),
            (0x0006_8041, "Panorama", "Sweep Panorama"),
            (0x0000_0007, "Portrait", "Portrait"),
            (0x0005_8011, "Sports", "Sports Action"),
            (0x0005_8015, "Macro", "Macro"),
            (0x0005_8014, "Landscape", "Landscape"),
            (0x0005_8012, "Sunset", "Sunset"),
            (0x0005_8013, "Night scene", "Night Scene"),
            (0x0005_8016, "Twilight", "Hand-held Twilight"),
            (0x0005_8017, "Night portrait", "Night Portrait"),
            (0x0005_8018, "Anti blur", "Anti Motion Blur"),
        ];
        for (raw, short, name) in modes {
            let got = label_for(PROP_EXPOSURE_PROGRAM, &PtpValue::U32(raw)).unwrap();
            assert_eq!(
                (got.short.as_str(), got.label.as_str()),
                (short, name),
                "{raw:#010x}"
            );
        }
        // Older bodies send the same codes in 16 bits.
        let old = label_for(PROP_EXPOSURE_PROGRAM, &PtpValue::U16(0x8050)).unwrap();
        assert_eq!(old.short, "Movie P");
    }

    /// Every drive mode the real ZV-E10 lists.
    #[test]
    fn the_zv_e10s_own_drive_values_all_have_names() {
        let drives: [(u32, &str); 36] = [
            (0x0000_0001, "Single"),
            (0x0001_8012, "Continuous Lo"),
            (0x0001_8015, "Continuous Mid"),
            (0x0001_0002, "Continuous Hi"),
            (0x0001_8010, "Continuous Hi+"),
            (0x0003_8004, "Self-timer 10 s"),
            (0x0003_8003, "Self-timer 5 s"),
            (0x0003_8005, "Self-timer 2 s"),
            (0x0008_8008, "Self-timer 10 s, 3 shots"),
            (0x0008_8009, "Self-timer 10 s, 5 shots"),
            (0x0008_800C, "Self-timer 5 s, 3 shots"),
            (0x0008_800D, "Self-timer 5 s, 5 shots"),
            (0x0008_800E, "Self-timer 2 s, 3 shots"),
            (0x0008_800F, "Self-timer 2 s, 5 shots"),
            (0x0004_8337, "Bracket 0.3 EV, 3 shots, continuous"),
            (0x0004_8537, "Bracket 0.3 EV, 5 shots, continuous"),
            (0x0004_8357, "Bracket 0.5 EV, 3 shots, continuous"),
            (0x0004_8557, "Bracket 0.5 EV, 5 shots, continuous"),
            (0x0004_8377, "Bracket 0.7 EV, 3 shots, continuous"),
            (0x0004_8577, "Bracket 0.7 EV, 5 shots, continuous"),
            (0x0004_8311, "Bracket 1.0 EV, 3 shots, continuous"),
            (0x0004_8321, "Bracket 2.0 EV, 3 shots, continuous"),
            (0x0004_8331, "Bracket 3.0 EV, 3 shots, continuous"),
            (0x0005_8336, "Bracket 0.3 EV, 3 shots, single"),
            (0x0005_8536, "Bracket 0.3 EV, 5 shots, single"),
            (0x0005_8356, "Bracket 0.5 EV, 3 shots, single"),
            (0x0005_8556, "Bracket 0.5 EV, 5 shots, single"),
            (0x0005_8376, "Bracket 0.7 EV, 3 shots, single"),
            (0x0005_8576, "Bracket 0.7 EV, 5 shots, single"),
            (0x0005_8310, "Bracket 1.0 EV, 3 shots, single"),
            (0x0005_8320, "Bracket 2.0 EV, 3 shots, single"),
            (0x0005_8330, "Bracket 3.0 EV, 3 shots, single"),
            (0x0006_8028, "White balance bracket Hi"),
            (0x0006_8018, "White balance bracket Lo"),
            (0x0007_8029, "DRO bracket Hi"),
            (0x0007_8019, "DRO bracket Lo"),
        ];
        for (raw, name) in drives {
            let got = label_for(PROP_STILL_CAPTURE_MODE, &PtpValue::U32(raw)).unwrap();
            assert_eq!(got.label, name, "{raw:#010x}");
            assert!(!got.label.contains("Unknown"), "{raw:#010x}");
        }
        // While the camera has the drive mode switched off it reports 0.
        let off = label_for(PROP_STILL_CAPTURE_MODE, &PtpValue::U32(0)).unwrap();
        assert_eq!(off.label, "—");
        // Newer bodies: two-shot brackets and the finer steps.
        assert_eq!(
            drive_label(0xC237),
            "Bracket 0.3 EV, 2 shots plus, continuous"
        );
        assert_eq!(drive_label(0xC23E), "Bracket 0.3 EV, 2 shots minus, single");
        assert_eq!(drive_label(0x8741), "Bracket 1.3 EV, 7 shots, continuous");
        assert_eq!(drive_label(0x8011), "Continuous Hi, live");
        assert_eq!(drive_label(0x8040), "Focus bracket");
    }

    #[test]
    fn long_names_have_a_compact_form_for_the_chips() {
        let drive = |raw: u32| {
            label_for(PROP_STILL_CAPTURE_MODE, &PtpValue::U32(raw))
                .unwrap()
                .short
        };
        assert_eq!(drive(0x0000_0001), "Single");
        assert_eq!(drive(0x0001_8012), "Cont. Lo");
        assert_eq!(drive(0x0001_8010), "Cont. Hi+");
        assert_eq!(drive(0x0003_8004), "Timer 10 s");
        assert_eq!(drive(0x0008_800C), "Timer 5 s ×3");
        assert_eq!(drive(0x0004_8337), "BRK C 0.3 ×3");
        assert_eq!(drive(0x0005_8320), "BRK S 2.0 ×3");
        assert_eq!(drive(0x0006_8028), "WB BRK Hi");
        assert_eq!(drive(0x0007_8019), "DRO BRK Lo");
        assert_eq!(drive(0), "—");
        assert_eq!(wb_short(0x8001), "Fluor. warm");
        assert_eq!(wb_short(0x8030), "Underwater");
        assert_eq!(wb_short(0x0004), "Daylight");
        // Nothing on a chip is wider than a dozen characters.
        for raw in [0x8337u16, 0x8936, 0xC23F, 0x800D, 0x8017, 0x8014, 0x8029] {
            assert!(
                drive_short(raw).chars().count() <= 13,
                "{}",
                drive_short(raw)
            );
        }
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
