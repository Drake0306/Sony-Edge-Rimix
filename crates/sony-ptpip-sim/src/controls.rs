//! The simulator's answers to 0x9207 controls: shutter buttons, movie record
//! and stepped property writes.

use sony_ptpip::ptp::codes::*;
use sony_ptpip::ptp::PtpValue;
use sony_ptpip::sony::Form;
use tokio::task::AbortHandle;

use crate::props::PropStore;

const PRESS: u16 = 2;
const RELEASE: u16 = 1;

/// Button state and captures waiting to be fetched, kept across sessions.
#[derive(Default)]
pub struct Controls {
    full_pressed: bool,
    rec_pressed: bool,
    /// Pictures taken but not yet fetched; Task 9's GetObject pops these.
    capture_queue: u32,
    /// Timer that reports focus found while the half press is held.
    focus_timer: Option<AbortHandle>,
}

/// What a control did, for the session to act on.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Nothing visible changed.
    Unchanged,
    /// This property's current value changed.
    Changed(u16),
    /// The half press began; the session starts the focus timer.
    StartFocus,
}

impl Controls {
    /// Applies one 0x9207 control. `Err` holds the response code.
    pub fn apply(&mut self, props: &mut PropStore, code: u16, data: &[u8]) -> Result<Outcome, u16> {
        match code {
            CTRL_SHUTTER_HALF | CTRL_SHUTTER_FULL | CTRL_MOVIE_REC => {
                let button = match data {
                    [lo, hi] => u16::from_le_bytes([*lo, *hi]),
                    _ => return Err(RC_INVALID_PARAMETER),
                };
                if button != PRESS && button != RELEASE {
                    return Err(RC_INVALID_PARAMETER);
                }
                Ok(self.button(props, code, button == PRESS))
            }
            _ => match data {
                [direction] => step(props, code, *direction as i8),
                _ => Err(RC_INVALID_PARAMETER),
            },
        }
    }

    fn button(&mut self, props: &mut PropStore, code: u16, pressed: bool) -> Outcome {
        match (code, pressed) {
            (CTRL_SHUTTER_HALF, true) => Outcome::StartFocus,
            (CTRL_SHUTTER_HALF, false) => {
                if let Some(timer) = self.focus_timer.take() {
                    timer.abort();
                }
                changed(
                    props,
                    PROP_FOCUS_FOUND,
                    flag(props, PROP_FOCUS_FOUND, false),
                )
            }
            (CTRL_SHUTTER_FULL, true) => {
                self.full_pressed = true;
                Outcome::Unchanged
            }
            (CTRL_SHUTTER_FULL, false) if std::mem::take(&mut self.full_pressed) => {
                self.capture_queue = self.capture_queue.saturating_add(1);
                let count = self.capture_queue.min(0x7FFF) as u16;
                changed(
                    props,
                    PROP_CAPTURED_READY,
                    Some(PtpValue::U16(0x8000 | count)),
                )
            }
            (CTRL_MOVIE_REC, true) => {
                self.rec_pressed = true;
                Outcome::Unchanged
            }
            (CTRL_MOVIE_REC, false) if std::mem::take(&mut self.rec_pressed) => {
                let recording = props
                    .get(PROP_MOVIE_REC_STATE)
                    .and_then(|p| p.current.as_u64())
                    .is_some_and(|v| v != 0);
                let value = flag(props, PROP_MOVIE_REC_STATE, !recording);
                changed(props, PROP_MOVIE_REC_STATE, value)
            }
            _ => Outcome::Unchanged,
        }
    }

    /// Remembers the running focus timer so a release can cancel it.
    pub fn set_focus_timer(&mut self, timer: AbortHandle) {
        if let Some(old) = self.focus_timer.replace(timer) {
            old.abort();
        }
    }

    /// 0 or 1 in the datatype of the focus-found property.
    pub fn focus_found_value(props: &PropStore) -> Option<PtpValue> {
        flag(props, PROP_FOCUS_FOUND, true)
    }
}

/// Moves a property to the neighbouring settable value, clamped at the ends.
fn step(props: &mut PropStore, code: u16, direction: i8) -> Result<Outcome, u16> {
    let prop = props.get(code).ok_or(RC_INVALID_PARAMETER)?;
    if !prop.writable {
        return Err(RC_ACCESS_DENIED);
    }
    let Form::Enum { settable, .. } = &prop.form else {
        return Err(RC_INVALID_PARAMETER);
    };
    let at = settable
        .iter()
        .position(|v| *v == prop.current)
        .ok_or(RC_INVALID_PARAMETER)?;
    let next = match direction.signum() {
        1 => (at + 1).min(settable.len() - 1),
        -1 => at.saturating_sub(1),
        _ => return Err(RC_INVALID_PARAMETER),
    };
    let value = settable[next].clone();
    Ok(changed(props, code, Some(value)))
}

/// Sets `code` to `value` and reports whether it changed.
fn changed(props: &mut PropStore, code: u16, value: Option<PtpValue>) -> Outcome {
    match value {
        Some(v) => {
            if props.set(code, v) {
                Outcome::Changed(code)
            } else {
                Outcome::Unchanged
            }
        }
        None => Outcome::Unchanged,
    }
}

/// 0 or 1 in the property's own integer datatype.
fn flag(props: &PropStore, code: u16, on: bool) -> Option<PtpValue> {
    let n = on as u8;
    Some(match props.get(code)?.current {
        PtpValue::I8(_) => PtpValue::I8(n as i8),
        PtpValue::U8(_) => PtpValue::U8(n),
        PtpValue::I16(_) => PtpValue::I16(n.into()),
        PtpValue::U16(_) => PtpValue::U16(n.into()),
        PtpValue::I32(_) => PtpValue::I32(n.into()),
        PtpValue::U32(_) => PtpValue::U32(n.into()),
        PtpValue::I64(_) => PtpValue::I64(n.into()),
        PtpValue::U64(_) => PtpValue::U64(n.into()),
        PtpValue::Str(_) | PtpValue::Array(_) => return None,
    })
}
