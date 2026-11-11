//! Building [`CameraState`] from the camera's property table, and working out
//! the value a step lands on.

use super::{CameraState, Choice, PropertySummary};
use crate::capture::DEST_COMPUTER;
use crate::error::Error;
use crate::ptp::codes::{
    PROP_BATTERY, PROP_FOCUS_FOUND, PROP_MOVIE_REC_STATE, PROP_STILL_DESTINATION,
    PROP_WHITE_BALANCE,
};
use crate::ptp::PtpValue;
use crate::quirks::quirks_for;
use crate::sony::decode::{label_for, wb_label};
use crate::sony::property::{Form, PropertyDesc};

/// Colour temperature in kelvin, shown with white balance 0x8007.
const PROP_COLOUR_TEMP: u16 = 0xD20F;

/// What the facade knows about the camera; [`CameraState`] is built from it.
#[derive(Debug, Default)]
pub(super) struct Snapshot {
    pub connected: bool,
    pub secured: bool,
    pub model: String,
    pub firmware: String,
    pub props: Vec<PropertyDesc>,
    pub live_view_fps: f32,
    pub last_shot_at: Option<u64>,
}

impl Snapshot {
    pub fn prop(&self, code: u16) -> Option<&PropertyDesc> {
        self.props.iter().find(|p| p.code == code)
    }

    /// Merges a property table read from the camera: replaces each property
    /// that differs (see [`differs`]), adds any not seen before and keeps
    /// any the read left out. Returns true if anything changed.
    pub fn merge(&mut self, read: Vec<PropertyDesc>) -> bool {
        let mut changed = false;
        for desc in read {
            match self.props.iter_mut().find(|p| p.code == desc.code) {
                Some(slot) => {
                    if differs(slot, &desc) {
                        *slot = desc;
                        changed = true;
                    }
                }
                None => {
                    self.props.push(desc);
                    changed = true;
                }
            }
        }
        changed
    }

    pub fn to_state(&self) -> CameraState {
        let number = |code| self.prop(code).and_then(|p| p.current.as_u64());
        let colour_temp = number(PROP_COLOUR_TEMP).and_then(|k| u16::try_from(k).ok());
        CameraState {
            connected: self.connected,
            secured: self.secured,
            model: self.model.clone(),
            firmware: self.firmware.clone(),
            generation: quirks_for(&self.model).generation.label().into(),
            battery: number(PROP_BATTERY)
                .and_then(|v| u8::try_from(v).ok())
                .filter(|&v| v <= 100),
            recording: number(PROP_MOVIE_REC_STATE).is_some_and(|v| v != 0),
            focus_found: number(PROP_FOCUS_FOUND).is_some_and(|v| v != 0),
            properties: self.props.iter().map(|p| summary(p, colour_temp)).collect(),
            live_view_fps: self.live_view_fps,
            last_shot_at: self.last_shot_at,
            shots_to_computer: number(PROP_STILL_DESTINATION).map(|v| v & DEST_COMPUTER != 0),
        }
    }
}

/// True when `new` would show differently from `old`: another current
/// value, enabled state, writability or set of allowed values.
fn differs(old: &PropertyDesc, new: &PropertyDesc) -> bool {
    old.current != new.current
        || old.enabled != new.enabled
        || old.writable != new.writable
        || old.form != new.form
}

fn summary(desc: &PropertyDesc, colour_temp: Option<u16>) -> PropertySummary {
    let (label, short) = labels(desc.code, &desc.current, colour_temp);
    let choices = match &desc.form {
        Form::Enum { settable, .. } => settable
            .iter()
            .map(|raw| Choice {
                raw: raw.clone(),
                label: labels(desc.code, raw, colour_temp).0,
            })
            .collect(),
        Form::Range { .. } | Form::None => Vec::new(),
    };
    PropertySummary {
        code: desc.code,
        label,
        short,
        raw: desc.current.clone(),
        writable: desc.writable,
        enabled: desc.enabled.clone(),
        choices,
    }
}

/// Full and short labels for a value; raw text when the code is unknown.
fn labels(code: u16, value: &PtpValue, colour_temp: Option<u16>) -> (String, String) {
    if code == PROP_WHITE_BALANCE {
        if let Some(raw) = value.as_u64().and_then(|v| u16::try_from(v).ok()) {
            let label = wb_label(raw, colour_temp);
            return (label.clone(), label);
        }
    }
    match label_for(code, value) {
        Some(l) => (l.label, l.short),
        None => {
            let raw = raw_text(value);
            (raw.clone(), raw)
        }
    }
}

fn raw_text(value: &PtpValue) -> String {
    match value {
        PtpValue::Str(s) => s.clone(),
        other => match other.as_i64() {
            Some(n) => n.to_string(),
            None => format!("{other:?}"),
        },
    }
}

/// The value `delta` places from the current one: through the settable list
/// for an enum, or `delta` steps through a range. Clamped at the ends.
pub(super) fn step_target(desc: &PropertyDesc, delta: i32) -> Result<PtpValue, Error> {
    match &desc.form {
        Form::Enum { settable, .. } => {
            let at = settable
                .iter()
                .position(|v| *v == desc.current)
                .ok_or(Error::Unsupported(
                    "current value is not in the settable list",
                ))?;
            let to = (at as i64 + i64::from(delta)).clamp(0, settable.len() as i64 - 1);
            Ok(settable[to as usize].clone())
        }
        Form::Range { min, max, step } => {
            let not_numeric = Error::Unsupported("range is not numeric");
            let (Some(min_n), Some(max_n), Some(step_n), Some(cur)) = (
                min.as_i64(),
                max.as_i64(),
                step.as_i64(),
                desc.current.as_i64(),
            ) else {
                return Err(not_numeric);
            };
            if step_n <= 0 || max_n < min_n {
                return Err(Error::Unsupported("range has no usable step"));
            }
            // The last value on the grid min, min + step, ... not above max.
            let top = min_n + (max_n - min_n) / step_n * step_n;
            let to = cur
                .saturating_add(i64::from(delta).saturating_mul(step_n))
                .clamp(min_n, top);
            // Snap onto the grid in case the current value sits off it.
            let to = min_n + (to - min_n) / step_n * step_n;
            same_type(&desc.current, to).ok_or(not_numeric)
        }
        Form::None => Err(Error::Unsupported("property lists no values")),
    }
}

/// How far one press of plus or minus moves the ISO.
const ISO_STEP: i64 = 50;
/// The ISO value meaning Auto.
const ISO_AUTO: u64 = 0x00FF_FFFF;

/// The ISO `delta` presses from the current one. A camera only takes the
/// stops it lists (100, 125, 160, ...), so each press aims [`ISO_STEP`]
/// away and lands on the listed stop nearest that, always moving at least
/// one stop; where the stops are wider apart than the step, that is the
/// next stop. Auto and the multi-frame noise reduction values are not
/// stepped onto: up from Auto starts at the lowest stop, down stays put.
pub(super) fn iso_step_target(desc: &PropertyDesc, delta: i32) -> Result<PtpValue, Error> {
    let Form::Enum { settable, .. } = &desc.form else {
        return step_target(desc, delta);
    };
    let mut stops: Vec<i64> = settable
        .iter()
        .filter_map(PtpValue::as_u64)
        .filter(|&v| v < ISO_AUTO)
        .map(|v| v as i64)
        .collect();
    stops.sort_unstable();
    stops.dedup();
    let Some(mut at) = desc.current.as_u64() else {
        return Err(Error::Unsupported("ISO is not a number"));
    };
    if stops.is_empty() {
        return step_target(desc, delta);
    }
    let up = delta > 0;
    for _ in 0..delta.unsigned_abs() {
        if at == ISO_AUTO {
            if !up {
                break;
            }
            at = stops[0] as u64;
            continue;
        }
        // Only plain values are stepped from; a flagged one keeps its number.
        let from = (at & ISO_AUTO) as i64;
        let aim = if up { from + ISO_STEP } else { from - ISO_STEP };
        let beyond = stops
            .iter()
            .copied()
            .filter(|&s| if up { s > from } else { s < from });
        let Some(next) = beyond.min_by_key(|&s| ((s - aim).abs(), (s - from).abs())) else {
            break;
        };
        at = next as u64;
    }
    same_type(&desc.current, at as i64).ok_or(Error::Unsupported("ISO is not a number"))
}

/// `n` as a value of the same integer type as `template`.
fn same_type(template: &PtpValue, n: i64) -> Option<PtpValue> {
    Some(match template {
        PtpValue::I8(_) => PtpValue::I8(n.try_into().ok()?),
        PtpValue::U8(_) => PtpValue::U8(n.try_into().ok()?),
        PtpValue::I16(_) => PtpValue::I16(n.try_into().ok()?),
        PtpValue::U16(_) => PtpValue::U16(n.try_into().ok()?),
        PtpValue::I32(_) => PtpValue::I32(n.try_into().ok()?),
        PtpValue::U32(_) => PtpValue::U32(n.try_into().ok()?),
        PtpValue::I64(_) => PtpValue::I64(n),
        PtpValue::U64(_) => PtpValue::U64(n.try_into().ok()?),
        PtpValue::Str(_) | PtpValue::Array(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ptp::codes::PROP_ISO;
    use crate::sony::property::Enabled;

    fn desc(current: PtpValue, form: Form) -> PropertyDesc {
        PropertyDesc {
            code: 0xD20F,
            datatype: 0x0004,
            writable: true,
            enabled: Enabled::On,
            default: current.clone(),
            current,
            form,
        }
    }

    fn range(current: u16) -> PropertyDesc {
        let form = Form::Range {
            min: PtpValue::U16(2500),
            max: PtpValue::U16(9950),
            step: PtpValue::U16(100),
        };
        desc(PtpValue::U16(current), form)
    }

    #[test]
    fn range_steps_clamp_to_the_grid() {
        assert_eq!(step_target(&range(5200), 3).unwrap(), PtpValue::U16(5500));
        assert_eq!(step_target(&range(2600), -5).unwrap(), PtpValue::U16(2500));
        // 9950 is off the grid; the highest reachable value is 9900.
        assert_eq!(step_target(&range(9800), 9).unwrap(), PtpValue::U16(9900));
    }

    #[test]
    fn enum_steps_clamp_at_both_ends() {
        let list = vec![PtpValue::U16(1), PtpValue::U16(2), PtpValue::U16(3)];
        let form = Form::Enum {
            settable: list,
            reportable: Vec::new(),
        };
        let d = desc(PtpValue::U16(2), form);
        assert_eq!(step_target(&d, -7).unwrap(), PtpValue::U16(1));
        assert_eq!(step_target(&d, 7).unwrap(), PtpValue::U16(3));
        assert_eq!(step_target(&d, 0).unwrap(), PtpValue::U16(2));
    }

    #[test]
    fn merge_reports_only_real_changes() {
        let mut snap = Snapshot::default();
        assert!(snap.merge(vec![range(5200)]));
        assert!(!snap.merge(vec![range(5200)]));
        assert!(!snap.merge(Vec::new()));
        assert!(snap.merge(vec![range(5300)]));
        assert_eq!(snap.props[0].current, PtpValue::U16(5300));
        let mut off = range(5300);
        off.enabled = Enabled::Off;
        assert!(snap.merge(vec![off.clone()]));
        off.writable = false;
        assert!(snap.merge(vec![off.clone()]));
        off.form = Form::None;
        assert!(snap.merge(vec![off]));
        let mut other = range(5300);
        other.code = 0x500E;
        assert!(snap.merge(vec![other]));
        assert_eq!(snap.props.len(), 2);
    }

    #[test]
    fn unknown_codes_fall_back_to_raw_numbers() {
        assert_eq!(
            labels(0xD20F, &PtpValue::U16(5200), None),
            ("5200".to_string(), "5200".to_string())
        );
        assert_eq!(
            labels(0x5005, &PtpValue::U16(0x8007), Some(5600)).0,
            "5600K"
        );
    }

    fn iso(current: u32) -> PropertyDesc {
        let stops = [
            0x00FF_FFFF,
            50,
            64,
            80,
            100,
            125,
            160,
            200,
            250,
            320,
            400,
            500,
            640,
            800,
            1000,
            1250,
            1600,
            3200,
            0x1000_0320,
        ];
        PropertyDesc {
            code: PROP_ISO,
            ..desc(
                PtpValue::U32(current),
                Form::Enum {
                    settable: stops.map(PtpValue::U32).to_vec(),
                    reportable: vec![],
                },
            )
        }
    }

    fn iso_after(current: u32, delta: i32) -> u64 {
        iso_step_target(&iso(current), delta)
            .unwrap()
            .as_u64()
            .unwrap()
    }

    #[test]
    fn iso_steps_about_fifty_onto_a_real_stop() {
        // 150 is nearer 160 than 125; 210 nearer 200; then 250, 320 (for 300).
        assert_eq!(iso_after(100, 1), 160);
        assert_eq!(iso_after(160, 1), 200);
        assert_eq!(iso_after(200, 1), 250);
        assert_eq!(iso_after(250, 1), 320);
        assert_eq!(iso_after(320, -1), 250);
        assert_eq!(iso_after(200, -1), 160);
        assert_eq!(iso_after(160, -1), 100);
        assert_eq!(iso_after(100, -1), 50);
    }

    #[test]
    fn iso_always_moves_at_least_one_stop_where_stops_are_wider_than_fifty() {
        assert_eq!(iso_after(400, 1), 500);
        assert_eq!(iso_after(800, 1), 1000);
        assert_eq!(iso_after(1600, 1), 3200);
        assert_eq!(iso_after(3200, -1), 1600);
        assert_eq!(iso_after(1000, -1), 800);
    }

    #[test]
    fn iso_stops_at_the_ends_and_leaves_auto_and_noise_reduction_values_alone() {
        assert_eq!(iso_after(3200, 1), 3200);
        assert_eq!(iso_after(50, -1), 50);
        // Several presses in one call.
        assert_eq!(iso_after(100, 3), 250);
        // From Auto, up starts at the lowest stop and down stays on Auto.
        assert_eq!(iso_after(0x00FF_FFFF, 1), 50);
        assert_eq!(iso_after(0x00FF_FFFF, -1), 0x00FF_FFFF);
        // A value off the list (the camera in an odd state) still moves.
        assert_eq!(iso_after(140, 1), 200);
    }
}
