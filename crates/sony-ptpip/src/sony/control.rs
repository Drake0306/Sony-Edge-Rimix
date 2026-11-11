//! Camera controls (0x9207): shutter, movie record, focus, and stepped
//! property writes.

use std::time::Duration;

use tokio::time::sleep;

use crate::error::Error;
use crate::ptp::codes::{
    CTRL_MOVIE_REC, CTRL_SHUTTER_FULL, CTRL_SHUTTER_HALF, OP_SDIO_CONTROL_DEVICE, RC_ACCESS_DENIED,
    RC_INVALID_PARAMETER,
};
use crate::ptp::PtpValue;
use crate::quirks::{Quirks, WriteMode};
use crate::sony::property::{read_all_props, set_property, Form, PropertyDesc};
use crate::PtpIpConnection;

const PRESS: PtpValue = PtpValue::U16(2);
const RELEASE: PtpValue = PtpValue::U16(1);
/// Longest number of single steps a stepped write may take.
pub(crate) const MAX_STEPS: usize = 40;
/// How many times, and how far apart, the value is read again after a
/// nudge before the camera is taken to be at its limit: a body applies a
/// nudge a moment after it answers.
const NUDGE_READS: usize = 6;
const NUDGE_WAIT: std::time::Duration = std::time::Duration::from_millis(70);

/// Sends one control (0x9207 [code]) with `value` as its data phase.
pub async fn control(conn: &mut PtpIpConnection, code: u16, value: &PtpValue) -> Result<(), Error> {
    let mut data = Vec::new();
    value.encode(&mut data);
    conn.command_data_out(OP_SDIO_CONTROL_DEVICE, &[code as u32], &data)
        .await?;
    Ok(())
}

/// Takes one picture. With `with_af` the shutter is half pressed first so the
/// camera can focus.
///
/// If any step fails after a press was sent, every button pressed so far is
/// released (best effort, errors ignored) before the first error is returned,
/// so the camera is not left with its shutter held down.
pub async fn shoot(conn: &mut PtpIpConnection, with_af: bool) -> Result<(), Error> {
    let mut held = Vec::new();
    let result = shoot_steps(conn, with_af, &mut held).await;
    release_on_error(conn, &held, result).await
}

async fn shoot_steps(
    conn: &mut PtpIpConnection,
    with_af: bool,
    held: &mut Vec<u16>,
) -> Result<(), Error> {
    if with_af {
        press(conn, CTRL_SHUTTER_HALF, held).await?;
        sleep(Duration::from_millis(500)).await;
    }
    press(conn, CTRL_SHUTTER_FULL, held).await?;
    sleep(Duration::from_millis(35)).await;
    release(conn, CTRL_SHUTTER_FULL, held).await?;
    if with_af {
        sleep(Duration::from_millis(1000)).await;
        release(conn, CTRL_SHUTTER_HALF, held).await?;
    }
    Ok(())
}

/// Half presses the shutter long enough to focus, then lets go. The half
/// press is released even if a step fails.
pub async fn focus(conn: &mut PtpIpConnection) -> Result<(), Error> {
    let mut held = Vec::new();
    let result = async {
        press(conn, CTRL_SHUTTER_HALF, &mut held).await?;
        sleep(Duration::from_millis(700)).await;
        release(conn, CTRL_SHUTTER_HALF, &mut held).await
    }
    .await;
    release_on_error(conn, &held, result).await
}

/// Starts movie recording if stopped, or stops it if recording. The button
/// is released even if a step fails.
pub async fn toggle_record(conn: &mut PtpIpConnection) -> Result<(), Error> {
    let mut held = Vec::new();
    let result = async {
        press(conn, CTRL_MOVIE_REC, &mut held).await?;
        sleep(Duration::from_millis(35)).await;
        release(conn, CTRL_MOVIE_REC, &mut held).await
    }
    .await;
    release_on_error(conn, &held, result).await
}

/// Presses button `code`. It counts as held from before the request goes
/// out, since a failed request may still have reached the camera.
async fn press(conn: &mut PtpIpConnection, code: u16, held: &mut Vec<u16>) -> Result<(), Error> {
    held.push(code);
    control(conn, code, &PRESS).await
}

/// Releases button `code` and stops counting it as held once the camera
/// accepted the release.
async fn release(conn: &mut PtpIpConnection, code: u16, held: &mut Vec<u16>) -> Result<(), Error> {
    control(conn, code, &RELEASE).await?;
    held.retain(|&c| c != code);
    Ok(())
}

/// On error, releases the buttons in `held`, last pressed first, ignoring
/// their errors, then returns the original error.
async fn release_on_error(
    conn: &mut PtpIpConnection,
    held: &[u16],
    result: Result<(), Error>,
) -> Result<(), Error> {
    if result.is_err() {
        for &code in held.iter().rev() {
            if let Err(e) = control(conn, code, &RELEASE).await {
                tracing::debug!(code, error = %e, "releasing a button after a failure failed");
            }
        }
    }
    result
}

/// Sets a property to `target`, which must be one the property lists.
///
/// In [`WriteMode::Direct`] the value is written with 0x9205; if the camera
/// refuses that with InvalidParameter or AccessDenied and the property has a
/// settable list, the value is reached by stepping instead. In
/// [`WriteMode::Step`] the value is always reached by stepping. Range-form
/// properties can only be written directly.
pub async fn set_value(
    conn: &mut PtpIpConnection,
    desc: &PropertyDesc,
    target: &PtpValue,
    quirks: &Quirks,
) -> Result<(), Error> {
    if !desc.accepts(target) {
        return Err(Error::Unsupported("value not allowed"));
    }
    match quirks.write_mode {
        WriteMode::Step => step_to(conn, desc, target).await,
        WriteMode::Direct => match set_property(conn, desc.code, target).await {
            Err(Error::Response { code, .. })
                if (code == RC_INVALID_PARAMETER || code == RC_ACCESS_DENIED)
                    && settable(desc).is_some_and(|list| list.contains(target)) =>
            {
                step_to(conn, desc, target).await
            }
            other => other,
        },
    }
}

/// Walks a property that lists no values to `target`, one nudge (0x9207
/// with +1 or -1) at a time, as libgphoto2 sets aperture and shutter speed
/// on these bodies (`_put_sony_value_u16`). +1 moves to the larger value.
///
/// It stops when the value reaches or passes `target`, so a target between
/// two of the lens's values ends on the first one beyond it, and when the
/// value no longer moves, which is the end of what the lens or body
/// allows. Neither is an error.
pub async fn nudge_to(
    conn: &mut PtpIpConnection,
    desc: &PropertyDesc,
    target: &PtpValue,
) -> Result<(), Error> {
    let not_numeric = Error::Unsupported("value is not a number");
    let target = target.as_i64().ok_or(not_numeric)?;
    let mut current = desc
        .current
        .as_i64()
        .ok_or(Error::Unsupported("value is not a number"))?;
    let up = target > current;
    for _ in 0..MAX_STEPS {
        if current == target || (up && current > target) || (!up && current < target) {
            return Ok(());
        }
        control(conn, desc.code, &PtpValue::I8(if up { 1 } else { -1 })).await?;
        let mut moved = None;
        for read in 0..NUDGE_READS {
            if read > 0 {
                tokio::time::sleep(NUDGE_WAIT).await;
            }
            let now = read_all_props(conn)
                .await?
                .into_iter()
                .find(|p| p.code == desc.code)
                .and_then(|p| p.current.as_i64());
            if now.is_some_and(|n| n != current) {
                moved = now;
                break;
            }
        }
        match moved {
            Some(now) => current = now,
            // The end of what the lens or body allows.
            None => return Ok(()),
        }
    }
    Ok(())
}

fn settable(desc: &PropertyDesc) -> Option<&[PtpValue]> {
    match &desc.form {
        Form::Enum { settable, .. } => Some(settable),
        _ => None,
    }
}

/// Moves the property one settable entry at a time until it reads `target`.
async fn step_to(
    conn: &mut PtpIpConnection,
    desc: &PropertyDesc,
    target: &PtpValue,
) -> Result<(), Error> {
    let list = settable(desc).ok_or(Error::Unsupported("step"))?;
    let index = |v: &PtpValue| list.iter().position(|x| x == v);
    let goal = index(target).ok_or(Error::Unsupported("step"))?;
    let mut current = desc.current.clone();
    for _ in 0..MAX_STEPS {
        if current == *target {
            return Ok(());
        }
        let at = index(&current).ok_or(Error::Unsupported("step"))?;
        let direction = if goal > at { 1 } else { -1 };
        control(conn, desc.code, &PtpValue::I8(direction)).await?;
        // The whole table, as libgphoto2 reads it; some bodies never answer
        // the single-property read (0x9251).
        current = read_all_props(conn)
            .await?
            .into_iter()
            .find(|p| p.code == desc.code)
            .ok_or_else(|| Error::Protocol(format!("property {:#06x} disappeared", desc.code)))?
            .current;
    }
    if current == *target {
        Ok(())
    } else {
        Err(Error::Unsupported("step limit"))
    }
}
