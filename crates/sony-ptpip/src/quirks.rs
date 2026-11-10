//! Per-model differences in how a camera wants to be driven.

use serde::{Deserialize, Serialize};

/// How property values are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// Write the value with 0x9205, stepping only if the camera refuses it.
    Direct,
    /// Nudge the value one list entry at a time with 0x9207.
    Step,
}

/// The camera generation, which decides how the body is driven over Wi-Fi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Generation {
    Gen3,
    Gen4,
}

impl Generation {
    /// The label shown in the UI: "3" or "4".
    pub fn label(self) -> &'static str {
        match self {
            Generation::Gen3 => "3",
            Generation::Gen4 => "4",
        }
    }
}

/// Static facts about a camera model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quirks {
    pub write_mode: WriteMode,
    pub live_view_via_http: bool,
    pub pairing: bool,
    pub generation: Generation,
    /// True when the body offers SSH (Access Authentication).
    pub access_authentication: bool,
    /// Properties known to need a specific write path. Empty for every model
    /// for now, meaning "try direct, then step"; a real-camera test can fill
    /// it in later.
    pub direct_write_properties: &'static [u16],
}

/// The quirks for a model name as the camera reports it. Unknown models get
/// the same defaults as the ZV-E10.
pub fn quirks_for(model: &str) -> Quirks {
    if GEN4_MODELS.contains(&model) {
        GEN4_QUIRKS
    } else {
        DEFAULT_QUIRKS
    }
}

/// Bodies driven the Gen4 way, with Access Authentication. The a7 IV and
/// FX3 only offer it from firmware 2.00; on older firmware discovery says
/// so and [`quirks_from`] turns it off.
pub const GEN4_MODELS: &[&str] = &[
    "ILCE-7M4",
    "ILCE-7RM5",
    "ILCE-7CR",
    "ILCE-7CM2",
    "ILCE-7M5",
    "ILCE-6700",
    "ILCE-9M3",
    "ILCE-1M2",
    "ZV-E1",
    "ZV-E10M2",
    "ILME-FX3",
    "ILME-FX30",
];

/// Like [`quirks_for`], but a known discovery answer about SSH wins over the
/// table.
pub fn quirks_from(model: &str, ssh_supported: Option<bool>) -> Quirks {
    let mut quirks = quirks_for(model);
    if let Some(ssh) = ssh_supported {
        quirks.access_authentication = ssh;
    }
    quirks
}

const DEFAULT_QUIRKS: Quirks = Quirks {
    write_mode: WriteMode::Direct,
    live_view_via_http: false,
    pairing: true,
    generation: Generation::Gen3,
    access_authentication: false,
    direct_write_properties: &[],
};

const GEN4_QUIRKS: Quirks = Quirks {
    generation: Generation::Gen4,
    access_authentication: true,
    ..DEFAULT_QUIRKS
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a7_iv_is_generation_four_with_ssh() {
        let q = quirks_for("ILCE-7M4");
        assert_eq!(q.generation, Generation::Gen4);
        assert!(q.access_authentication);
    }

    #[test]
    fn every_gen4_model_has_ssh() {
        for model in GEN4_MODELS {
            let q = quirks_for(model);
            assert_eq!(q.generation, Generation::Gen4, "{model}");
            assert!(q.access_authentication, "{model}");
        }
        for model in ["ILCE-7CR", "ZV-E10M2", "ILCE-9M3", "ILCE-1M2", "ILCE-7M5"] {
            assert!(GEN4_MODELS.contains(&model), "{model}");
        }
    }

    #[test]
    fn unknown_model_equals_zv_e10() {
        assert_eq!(quirks_for("Mystery"), quirks_for("ZV-E10"));
        assert_eq!(quirks_for("ZV-E10").generation, Generation::Gen3);
        assert!(!quirks_for("ZV-E10").access_authentication);
    }

    #[test]
    fn discovery_flag_overrides_ssh() {
        assert!(quirks_from("ZV-E10", Some(true)).access_authentication);
        assert!(!quirks_from("ILCE-7M4", Some(false)).access_authentication);
        assert_eq!(quirks_from("ILCE-7M4", None), quirks_for("ILCE-7M4"));
    }
}
