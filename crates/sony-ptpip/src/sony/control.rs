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
use crate::sony::property::{read_property, set_property, Form, PropertyDesc};
use crate::PtpIpConnection;

const PRESS: PtpValue = PtpValue::U16(2);
const RELEASE: PtpValue = PtpValue::U16(1);
/// Longest number of single steps a stepped write may take.
const MAX_STEPS: usize = 40;

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
pub async fn shoot(conn: &mut PtpIpConnection, with_af: bool) -> Result<(), Error> {
    if with_af {
        control(conn, CTRL_SHUTTER_HALF, &PRESS).await?;
        sleep(Duration::from_millis(500)).await;
    }
    control(conn, CTRL_SHUTTER_FULL, &PRESS).await?;
    sleep(Duration::from_millis(35)).await;
    control(conn, CTRL_SHUTTER_FULL, &RELEASE).await?;
    if with_af {
        sleep(Duration::from_millis(1000)).await;
        control(conn, CTRL_SHUTTER_HALF, &RELEASE).await?;
    }
    Ok(())
}

/// Half presses the shutter long enough to focus, then lets go.
pub async fn focus(conn: &mut PtpIpConnection) -> Result<(), Error> {
    control(conn, CTRL_SHUTTER_HALF, &PRESS).await?;
    sleep(Duration::from_millis(700)).await;
    control(conn, CTRL_SHUTTER_HALF, &RELEASE).await
}

/// Starts movie recording if stopped, or stops it if recording.
pub async fn toggle_record(conn: &mut PtpIpConnection) -> Result<(), Error> {
    control(conn, CTRL_MOVIE_REC, &PRESS).await?;
    sleep(Duration::from_millis(35)).await;
    control(conn, CTRL_MOVIE_REC, &RELEASE).await
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
        current = read_property(conn, desc.code).await?.current;
    }
    if current == *target {
        Ok(())
    } else {
        Err(Error::Unsupported("step limit"))
    }
}
