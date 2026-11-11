//! The simulator's property store, loaded from a JSON fixture.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;
use sony_ptpip::ptp::codes::*;
use sony_ptpip::ptp::PtpValue;
use sony_ptpip::sony::{encode_all_props, encode_property, Enabled, Form, PropertyDesc};

/// Resolves a relative fixture path against this crate's directory, so tests
/// in other crates find the bundled fixtures.
fn resolve(path: &Path) -> PathBuf {
    if path.is_relative() {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
    } else {
        path.to_path_buf()
    }
}

/// Loads a property fixture; relative paths are resolved against this crate.
pub fn load_fixture(path: &Path) -> Result<Vec<PropertyDesc>, String> {
    let path = resolve(path);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let json: Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    json.as_array()
        .ok_or("fixture must be a JSON array")?
        .iter()
        .map(|entry| {
            entry_from_json(entry).map_err(|e| format!("{}: {e}: {entry}", path.display()))
        })
        .collect()
}

/// A code or datatype: a JSON number or a "0x…" hex string.
fn code(v: &Value) -> Result<u16, String> {
    let n = match v {
        Value::String(s) => {
            let hex = s.strip_prefix("0x").ok_or("hex strings need a 0x prefix")?;
            u64::from_str_radix(hex, 16).map_err(|e| e.to_string())?
        }
        _ => v.as_u64().ok_or("expected a code")?,
    };
    u16::try_from(n).map_err(|_| "code out of range".to_string())
}

fn int<T: TryFrom<i64>>(v: &Value) -> Result<T, String> {
    let n = v.as_i64().ok_or("expected an integer")?;
    T::try_from(n).map_err(|_| format!("{n} out of range for the datatype"))
}

fn value(dt: u16, v: &Value) -> Result<PtpValue, String> {
    if dt & DT_ARRAY_FLAG != 0 && dt != DT_STR {
        let items = v.as_array().ok_or("expected an array")?;
        return items
            .iter()
            .map(|i| value(dt & !DT_ARRAY_FLAG, i))
            .collect::<Result<_, _>>()
            .map(PtpValue::Array);
    }
    Ok(match dt {
        DT_INT8 => PtpValue::I8(int(v)?),
        DT_UINT8 => PtpValue::U8(int(v)?),
        DT_INT16 => PtpValue::I16(int(v)?),
        DT_UINT16 => PtpValue::U16(int(v)?),
        DT_INT32 => PtpValue::I32(int(v)?),
        DT_UINT32 => PtpValue::U32(int(v)?),
        DT_INT64 => PtpValue::I64(int(v)?),
        DT_UINT64 => PtpValue::U64(v.as_u64().ok_or("expected an unsigned integer")?),
        DT_STR => PtpValue::Str(v.as_str().ok_or("expected a string")?.into()),
        _ => return Err(format!("unsupported datatype {dt:#06x}")),
    })
}

fn values(dt: u16, v: &Value) -> Result<Vec<PtpValue>, String> {
    v.as_array()
        .ok_or("expected a list")?
        .iter()
        .map(|i| value(dt, i))
        .collect()
}

fn form(dt: u16, v: &Value) -> Result<Form, String> {
    if v.as_str() == Some("none") {
        return Ok(Form::None);
    }
    if let Some(e) = v.get("enum") {
        return Ok(Form::Enum {
            settable: values(dt, &e["settable"])?,
            reportable: match e.get("reportable") {
                Some(r) => values(dt, r)?,
                None => Vec::new(),
            },
        });
    }
    if let Some(r) = v.get("range") {
        return Ok(Form::Range {
            min: value(dt, &r["min"])?,
            max: value(dt, &r["max"])?,
            step: value(dt, &r["step"])?,
        });
    }
    Err("form must be \"none\", {\"enum\": …} or {\"range\": …}".into())
}

fn entry_from_json(e: &Value) -> Result<PropertyDesc, String> {
    let datatype = code(&e["datatype"])?;
    Ok(PropertyDesc {
        code: code(&e["code"])?,
        datatype,
        writable: e["writable"].as_bool().ok_or("writable must be a bool")?,
        enabled: match e["enabled"].as_u64() {
            Some(0) => Enabled::Off,
            Some(1) => Enabled::On,
            Some(2) => Enabled::DisplayOnly,
            _ => return Err("enabled must be 0, 1 or 2".into()),
        },
        default: value(datatype, &e["default"])?,
        current: value(datatype, &e["current"])?,
        form: form(datatype, &e["form"])?,
    })
}

/// Why the simulator refuses a write.
#[derive(Debug, PartialEq)]
pub enum SetError {
    Unknown,
    ReadOnly,
    NotAllowed,
}

impl SetError {
    pub fn response_code(&self) -> u16 {
        match self {
            SetError::ReadOnly => RC_ACCESS_DENIED,
            SetError::Unknown | SetError::NotAllowed => RC_INVALID_PARAMETER,
        }
    }
}

/// The camera's current properties plus what changed since the last
/// 0x9209 mode 1 read.
pub struct PropStore {
    props: BTreeMap<u16, PropertyDesc>,
    changed: BTreeSet<u16>,
    /// Properties reported without their list of values.
    unlisted: Vec<u16>,
}

impl PropStore {
    pub fn new(props: Vec<PropertyDesc>) -> Self {
        PropStore {
            props: props.into_iter().map(|p| (p.code, p)).collect(),
            changed: BTreeSet::new(),
            unlisted: Vec::new(),
        }
    }

    /// Reports `codes` without their lists of values from now on.
    pub fn keep_lists_of(&mut self, codes: &[u16]) {
        self.unlisted = codes.to_vec();
    }

    pub fn is_unlisted(&self, code: u16) -> bool {
        self.unlisted.contains(&code)
    }

    /// `prop` as the camera reports it: without its list when it is unlisted.
    fn reported(&self, prop: &PropertyDesc) -> PropertyDesc {
        let mut out = prop.clone();
        if self.is_unlisted(prop.code) {
            out.form = Form::None;
        }
        out
    }

    pub fn codes(&self) -> Vec<u16> {
        self.props.keys().copied().collect()
    }

    pub fn get(&self, code: u16) -> Option<&PropertyDesc> {
        self.props.get(&code)
    }

    /// 0x9209 mode 0: every property.
    pub fn encode_all(&self) -> Vec<u8> {
        encode_all_props(
            &self
                .props
                .values()
                .map(|p| self.reported(p))
                .collect::<Vec<_>>(),
        )
    }

    /// 0x9209 mode 1: properties changed since the previous call.
    pub fn take_changed(&mut self) -> Vec<u8> {
        let changed: Vec<_> = std::mem::take(&mut self.changed)
            .into_iter()
            .filter_map(|c| self.props.get(&c).map(|p| self.reported(p)))
            .collect();
        encode_all_props(&changed)
    }

    /// 0x9251: one entry without a count, or `None` for an unknown code.
    pub fn encode_one(&self, code: u16) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        encode_property(self.props.get(&code)?, &mut out);
        Some(out)
    }

    /// 0x9205: decodes and validates a value written by the host. Returns
    /// whether the current value changed.
    pub fn write(&mut self, code: u16, data: &[u8]) -> Result<bool, SetError> {
        let prop = self.props.get(&code).ok_or(SetError::Unknown)?;
        if !prop.writable {
            return Err(SetError::ReadOnly);
        }
        // A setting that lists no values is only ever stepped.
        if self.is_unlisted(code) {
            return Err(SetError::NotAllowed);
        }
        let mut pos = 0;
        let value =
            PtpValue::decode(prop.datatype, data, &mut pos).map_err(|_| SetError::NotAllowed)?;
        if pos != data.len() || !prop.accepts(&value) {
            return Err(SetError::NotAllowed);
        }
        Ok(self.set(code, value))
    }

    /// Sets a value as if the camera changed it itself. Returns whether the
    /// property exists and its current value changed.
    pub fn set(&mut self, code: u16, value: PtpValue) -> bool {
        let Some(prop) = self.props.get_mut(&code) else {
            return false;
        };
        if prop.current == value {
            return false;
        }
        prop.current = value;
        self.changed.insert(code);
        true
    }
}
