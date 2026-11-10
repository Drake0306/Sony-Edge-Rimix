//! Per-model differences in how a camera wants to be driven.

/// How property values are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// Write the value with 0x9205, stepping only if the camera refuses it.
    Direct,
    /// Nudge the value one list entry at a time with 0x9207.
    Step,
}

/// Static facts about a camera model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quirks {
    pub write_mode: WriteMode,
    pub live_view_via_http: bool,
    pub pairing: bool,
}

/// The quirks for a model name as the camera reports it. Unknown models get
/// the same defaults as the ZV-E10.
pub fn quirks_for(model: &str) -> Quirks {
    match model {
        // Listed so the known model is explicit; it uses the defaults.
        "ZV-E10" => DEFAULT_QUIRKS,
        _ => DEFAULT_QUIRKS,
    }
}

const DEFAULT_QUIRKS: Quirks = Quirks {
    write_mode: WriteMode::Direct,
    live_view_via_http: false,
    pairing: true,
};
