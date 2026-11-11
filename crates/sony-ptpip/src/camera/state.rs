//! Building [`CameraState`] from the camera's property table, and working out
//! the value a step lands on.

use super::{CameraState, Choice, PropertySummary, ValueRange};
use crate::capture::DEST_COMPUTER;
use crate::error::Error;
use crate::ptp::codes::{
    PROP_BATTERY, PROP_EXPOSURE_PROGRAM, PROP_FOCUS_FOUND, PROP_MOVIE_REC_STATE,
    PROP_STILL_DESTINATION, PROP_WHITE_BALANCE,
};
use crate::ptp::PtpValue;
use crate::quirks::quirks_for;
use crate::sony::decode::{label_for, wb_label, wb_short};
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
    /// The last shooting mode reported that is one the camera lists: see
    /// [`Snapshot::mode_shown`].
    listed_mode: Option<u64>,
    /// Mode values reported that the camera does not list, each said in
    /// the log once.
    unlisted_modes: Vec<u64>,
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
        self.note_mode();
        changed
    }

    /// Remembers the mode while it is one the camera lists, and says once
    /// in the log when it reports one it does not list.
    fn note_mode(&mut self) {
        let Some((current, listed)) = self.mode_and_list() else {
            return;
        };
        if listed.contains(&current) {
            // Also when it is another family's: a remembered Superior Auto
            // must not come back after the camera has been somewhere else.
            self.listed_mode = Some(current);
        } else if !self.unlisted_modes.contains(&current) {
            self.unlisted_modes.push(current);
            let shown = self.mode_shown().and_then(|v| v.as_u64());
            tracing::info!(
                reported = %format_args!("{current:#010x}"),
                shown_as = %format_args!("{:#010x}", shown.unwrap_or(current)),
                "the camera reports a shooting mode it does not list (a scene it recognised in an automatic mode)"
            );
        }
    }

    /// The mode's current value and the values the camera lists for it.
    fn mode_and_list(&self) -> Option<(u64, Vec<u64>)> {
        let desc = self.prop(PROP_EXPOSURE_PROGRAM)?;
        let Form::Enum { settable, .. } = &desc.form else {
            return None;
        };
        let listed = settable.iter().filter_map(PtpValue::as_u64).collect();
        Some((desc.current.as_u64()?, listed))
    }

    /// The shooting mode to show instead of the one reported, when the
    /// camera reports one it does not list.
    ///
    /// In Intelligent Auto (and Superior Auto, and movie Intelligent Auto)
    /// the camera recognises the scene and puts it in the mode value: the
    /// automatic mode's family number in the upper 16 bits, the scene's code
    /// in the lower (`0x0004_8015` for Auto seeing a close subject, where
    /// the Macro scene mode itself is `0x0005_8015`). The mode has not
    /// changed, so the listed mode of that family is shown: the one last
    /// seen while the camera stayed in the family, else its automatic one.
    fn mode_shown(&self) -> Option<PtpValue> {
        let (current, listed) = self.mode_and_list()?;
        // Bodies that send 16 bits have no family to go by.
        if listed.contains(&current) || current <= 0xFFFF {
            return None;
        }
        let family = current >> 16;
        let in_family = |v: &u64| v >> 16 == family;
        let pick = self
            .listed_mode
            .filter(|m| in_family(m) && listed.contains(m))
            .or_else(|| {
                let auto = |v: &&u64| matches!(**v & 0xFFFF, 0x8000 | 0x8054 | 0x805D);
                listed.iter().filter(|v| in_family(v)).find(auto).copied()
            })
            .or_else(|| listed.iter().find(|v| in_family(v)).copied())?;
        same_type(&self.prop(PROP_EXPOSURE_PROGRAM)?.current, pick as i64)
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
            properties: self
                .props
                .iter()
                .map(|p| match self.mode_shown() {
                    Some(shown) if p.code == PROP_EXPOSURE_PROGRAM => {
                        let mut as_shown = p.clone();
                        as_shown.current = shown;
                        summary(&as_shown, colour_temp)
                    }
                    _ => summary(p, colour_temp),
                })
                .collect(),
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
            .map(|raw| {
                let (label, short) = labels(desc.code, raw, colour_temp);
                Choice {
                    raw: raw.clone(),
                    label,
                    short,
                }
            })
            .collect(),
        Form::Range { .. } | Form::None => Vec::new(),
    };
    let range = match &desc.form {
        Form::Range { min, max, step } => match (min.as_i64(), max.as_i64(), step.as_i64()) {
            (Some(min), Some(max), Some(step)) => Some(ValueRange { min, max, step }),
            _ => None,
        },
        _ => None,
    };
    PropertySummary {
        code: desc.code,
        label,
        short,
        raw: desc.current.clone(),
        writable: desc.writable,
        enabled: desc.enabled.clone(),
        choices,
        range,
    }
}

/// Full and short labels for a value; raw text when the code is unknown.
fn labels(code: u16, value: &PtpValue, colour_temp: Option<u16>) -> (String, String) {
    if code == PROP_WHITE_BALANCE {
        if let Some(raw) = value.as_u64().and_then(|v| u16::try_from(v).ok()) {
            let label = wb_label(raw, colour_temp);
            // A colour temperature shows as its kelvin value on the chip too.
            let short = if colour_temp.is_some() && label.ends_with('K') {
                label.clone()
            } else {
                wb_short(raw)
            };
            return (label, short);
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

/// The ISO value meaning Auto.
const ISO_AUTO: u64 = 0x00FF_FFFF;

/// The ISO `delta` presses from the current one: one of the camera's own
/// stops per press (100, 125, 160, ...), in order of value. A camera takes
/// no ISO between its stops, so this is the smallest change there is. Auto
/// and the multi-frame noise reduction values are not stepped onto: up
/// from Auto starts at the lowest stop, down stays put.
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
        let next = if up {
            stops.iter().copied().find(|&s| s > from)
        } else {
            stops.iter().copied().rev().find(|&s| s < from)
        };
        let Some(next) = next else {
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
    fn iso_steps_one_of_the_cameras_stops_at_a_time() {
        // Nothing smaller exists on a camera, and none is skipped.
        assert_eq!(iso_after(100, 1), 125);
        assert_eq!(iso_after(125, 1), 160);
        assert_eq!(iso_after(160, -1), 125);
        assert_eq!(iso_after(100, -1), 80);
        assert_eq!(iso_after(800, 1), 1000);
        assert_eq!(iso_after(1600, 1), 3200);
        assert_eq!(iso_after(3200, -1), 1600);
    }

    #[test]
    fn iso_stops_at_the_ends_and_leaves_auto_and_noise_reduction_values_alone() {
        assert_eq!(iso_after(3200, 1), 3200);
        assert_eq!(iso_after(50, -1), 50);
        // Several presses in one call.
        assert_eq!(iso_after(100, 3), 200);
        // From Auto, up starts at the lowest stop and down stays on Auto.
        assert_eq!(iso_after(0x00FF_FFFF, 1), 50);
        assert_eq!(iso_after(0x00FF_FFFF, -1), 0x00FF_FFFF);
        // A value off the list (the camera in an odd state) still moves.
        assert_eq!(iso_after(140, 1), 160);
        assert_eq!(iso_after(140, -1), 125);
    }

    /// The real ZV-E10's mode list, shortened.
    fn mode(current: u32) -> PropertyDesc {
        let listed = [
            0x0001_0002u32,
            0x0000_0001,
            0x0007_8050,
            0x0007_8054,
            0x0009_8059,
            0x0004_8000,
            0x0004_8001,
            0x0005_8015,
            0x0005_8011,
        ];
        PropertyDesc {
            code: PROP_EXPOSURE_PROGRAM,
            datatype: 0x0006,
            ..desc(
                PtpValue::U32(current),
                Form::Enum {
                    settable: listed.map(PtpValue::U32).to_vec(),
                    reportable: vec![],
                },
            )
        }
    }

    fn shown(snap: &Snapshot) -> (PtpValue, String) {
        let state = snap.to_state();
        let p = state
            .properties
            .iter()
            .find(|p| p.code == PROP_EXPOSURE_PROGRAM)
            .unwrap();
        (p.raw.clone(), p.label.clone())
    }

    /// In Intelligent Auto the camera recognises the scene and reports it in
    /// the mode value: Auto's family number with the scene's code under it.
    /// Roy saw the mode jump to Macro whenever the camera looked at
    /// something close. The mode is still Intelligent Auto.
    #[test]
    fn a_scene_the_camera_recognises_in_auto_does_not_change_the_mode_shown() {
        let mut snap = Snapshot::default();
        snap.merge(vec![mode(0x0004_8000)]);
        assert_eq!(
            shown(&snap),
            (PtpValue::U32(0x0004_8000), "Intelligent Auto".into())
        );
        // Auto family, Macro's code: recognised, not chosen.
        snap.merge(vec![mode(0x0004_8015)]);
        assert_eq!(
            shown(&snap),
            (PtpValue::U32(0x0004_8000), "Intelligent Auto".into())
        );
        // The same in Superior Auto stays Superior Auto.
        snap.merge(vec![mode(0x0004_8001)]);
        snap.merge(vec![mode(0x0004_8011)]);
        assert_eq!(
            shown(&snap),
            (PtpValue::U32(0x0004_8001), "Superior Auto".into())
        );
    }

    #[test]
    fn the_same_holds_in_movie_auto_and_a_chosen_scene_is_still_a_scene() {
        let mut snap = Snapshot::default();
        // Straight into a recognised scene, with nothing seen before it.
        snap.merge(vec![mode(0x0007_8015)]);
        assert_eq!(
            shown(&snap),
            (
                PtpValue::U32(0x0007_8054),
                "Intelligent Auto (Movie)".into()
            )
        );
        // Macro chosen from Scene Selection is one of the camera's own modes.
        snap.merge(vec![mode(0x0005_8015)]);
        assert_eq!(shown(&snap), (PtpValue::U32(0x0005_8015), "Macro".into()));
        // Leaving Auto and coming back does not show a stale Superior Auto.
        snap.merge(vec![mode(0x0004_8001)]);
        snap.merge(vec![mode(0x0001_0002)]);
        snap.merge(vec![mode(0x0004_8013)]);
        assert_eq!(shown(&snap).1, "Intelligent Auto");
    }
}
