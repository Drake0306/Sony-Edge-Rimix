//! The app's own settings, stored as JSON in the user's config folder.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sony_ptpip::capture::{Destination, DestinationMode};

/// How many cameras the connect screen remembers.
pub const MAX_RECENT: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Appearance {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Layout {
    #[default]
    Dock,
    Ring,
}

/// A camera connected to before, newest first in [`AppSettings::recent_cameras`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentCamera {
    /// What was typed or picked: an IP address, or `ip:port`.
    pub ip: String,
    pub model: String,
    /// Milliseconds since the Unix epoch.
    pub last_used: u64,
}

/// Everything the app remembers between runs. Missing fields take their
/// defaults, so older files still load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub appearance: Appearance,
    pub layout: Layout,
    pub destination_mode: DestinationMode,
    pub destination_base: PathBuf,
    pub recent_cameras: Vec<RecentCamera>,
    pub last_ip: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            appearance: Appearance::default(),
            layout: Layout::default(),
            destination_mode: DestinationMode::FolderPerDate,
            destination_base: default_destination_base(),
            recent_cameras: Vec::new(),
            last_ip: None,
        }
    }
}

/// `~/Pictures/Rimix`, or `~/Rimix` where there is no pictures folder.
fn default_destination_base() -> PathBuf {
    let dirs = directories::UserDirs::new();
    let base = dirs
        .as_ref()
        .and_then(|d| d.picture_dir().map(Path::to_path_buf))
        .or_else(|| dirs.as_ref().map(|d| d.home_dir().to_path_buf()))
        .unwrap_or_default();
    base.join("Rimix")
}

impl AppSettings {
    /// Where shots from the camera are saved.
    pub fn destination(&self) -> Destination {
        Destination {
            base: self.destination_base.clone(),
            mode: self.destination_mode.clone(),
            recovery_dir: None,
        }
    }

    /// Puts `ip` first in the recent list (dropping any older entry for it),
    /// keeps at most [`MAX_RECENT`] and makes it the last IP.
    pub fn remember_camera(&mut self, ip: &str, model: &str, now_millis: u64) {
        self.recent_cameras.retain(|c| c.ip != ip);
        self.recent_cameras.insert(
            0,
            RecentCamera {
                ip: ip.to_string(),
                model: model.to_string(),
                last_used: now_millis,
            },
        );
        self.recent_cameras.truncate(MAX_RECENT);
        self.last_ip = Some(ip.to_string());
    }
}

/// The platform's config folder for Rimix, which also holds the host GUID
/// file the command-line tool uses, so both share one camera pairing.
pub fn config_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "Rimix").map(|dirs| dirs.config_dir().to_path_buf())
}

/// Reads the settings at `path`; defaults when the file is missing or cannot
/// be parsed (the latter is logged).
pub fn load(path: &Path) -> AppSettings {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return AppSettings::default(),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "cannot read settings; using defaults");
            return AppSettings::default();
        }
    };
    serde_json::from_str(&text).unwrap_or_else(|e| {
        tracing::warn!(path = %path.display(), error = %e, "cannot parse settings; using defaults");
        AppSettings::default()
    })
}

/// Writes the settings to `path` through a temporary file and a rename, so a
/// crash never leaves half a file. Creates the parent folders.
pub fn save(path: &Path, settings: &AppSettings) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(settings)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let s = load(&tmp.path().join("nope").join("settings.json"));
        assert_eq!(s, AppSettings::default());
        assert_eq!(s.appearance, Appearance::Dark);
        assert_eq!(s.layout, Layout::Dock);
        assert_eq!(s.destination_mode, DestinationMode::FolderPerDate);
        assert!(s.destination_base.ends_with("Rimix"));
        assert!(s.recent_cameras.is_empty());
        assert_eq!(s.last_ip, None);
    }

    #[test]
    fn defaults_when_unparsable() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load(&path), AppSettings::default());
    }

    #[test]
    fn roundtrip_save_load() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a").join("b").join("settings.json");
        let mut s = AppSettings {
            appearance: Appearance::Light,
            layout: Layout::Ring,
            destination_mode: DestinationMode::LightroomWatched,
            destination_base: tmp.path().join("shots"),
            ..AppSettings::default()
        };
        s.remember_camera("192.168.122.1", "ZV-E10", 1_000);
        save(&path, &s).unwrap();
        assert_eq!(load(&path), s);
        // Only the settings file is left behind, not a temporary one.
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("settings.json")]);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"destinationBase\""), "{text}");
        assert!(text.contains("\"recentCameras\""), "{text}");
    }

    #[test]
    fn missing_fields_take_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        std::fs::write(&path, r#"{"layout":"Ring"}"#).unwrap();
        let s = load(&path);
        assert_eq!(s.layout, Layout::Ring);
        assert_eq!(s.appearance, Appearance::Dark);
    }

    #[test]
    fn recent_cameras_are_newest_first_unique_and_capped() {
        let mut s = AppSettings::default();
        for n in 0..10 {
            s.remember_camera(&format!("10.0.0.{n}"), "ILCE-7M4", n);
        }
        s.remember_camera("10.0.0.5", "ILCE-7M4", 99);
        assert_eq!(s.recent_cameras.len(), MAX_RECENT);
        assert_eq!(s.recent_cameras[0].ip, "10.0.0.5");
        assert_eq!(s.recent_cameras[0].last_used, 99);
        assert_eq!(s.recent_cameras[1].ip, "10.0.0.9");
        assert_eq!(
            s.recent_cameras
                .iter()
                .filter(|c| c.ip == "10.0.0.5")
                .count(),
            1
        );
        assert_eq!(s.last_ip.as_deref(), Some("10.0.0.5"));
    }
}
