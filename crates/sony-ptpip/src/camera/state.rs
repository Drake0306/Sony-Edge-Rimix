//! Building [`CameraState`] from the camera's property table, and working out
//! the value a step lands on.

use super::{CameraState, Choice, PropertySummary};
use crate::error::Error;
use crate::ptp::codes::{PROP_BATTERY, PROP_FOCUS_FOUND, PROP_MOVIE_REC_STATE, PROP_WHITE_BALANCE};
use crate::ptp::PtpValue;
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

    /// Replaces each changed property, adding any not seen before.
    pub fn merge(&mut self, changed: Vec<PropertyDesc>) {
        for desc in changed {
            match self.props.iter_mut().find(|p| p.code == desc.code) {
                Some(slot) => *slot = desc,
                None => self.props.push(desc),
            }
        }
    }

    pub fn to_state(&self) -> CameraState {
        let number = |code| self.prop(code).and_then(|p| p.current.as_u64());
        let colour_temp = number(PROP_COLOUR_TEMP).and_then(|k| u16::try_from(k).ok());
        CameraState {
            connected: self.connected,
            secured: self.secured,
            model: self.model.clone(),
            firmware: self.firmware.clone(),
            battery: number(PROP_BATTERY)
                .and_then(|v| u8::try_from(v).ok())
                .filter(|&v| v <= 100),
            recording: number(PROP_MOVIE_REC_STATE).is_some_and(|v| v != 0),
            focus_found: number(PROP_FOCUS_FOUND).is_some_and(|v| v != 0),
            properties: self.props.iter().map(|p| summary(p, colour_temp)).collect(),
            live_view_fps: self.live_view_fps,
            last_shot_at: self.last_shot_at,
        }
    }
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
}
