//! The app's own settings, stored as JSON in the user's config folder.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sony_ptpip::capture::{Destination, DestinationMode};
use sony_ptpip::ptp::PtpValue;

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

/// Whether the app joins a camera's Wi-Fi network by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WifiJoin {
    #[default]
    Automatic,
    Ask,
}

/// One camera property inside a [`Preset`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetValue {
    /// The PTP device property code.
    pub code: u16,
    pub raw: PtpValue,
}

/// A saved set of camera properties for one camera model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub model: String,
    /// Which of the four preset slots, 1 to 4.
    pub slot: u8,
    pub values: Vec<PresetValue>,
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
    /// The camera's Wi-Fi network name, when known. Never its password.
    #[serde(default)]
    pub ssid: Option<String>,
    /// The Access Authentication username, for cameras reached through SSH.
    /// Never the password.
    #[serde(default)]
    pub ssh_user: Option<String>,
    /// The SSH host key fingerprint the user confirmed for this camera.
    #[serde(default)]
    pub ssh_fingerprint: Option<String>,
}

/// The receiver the camera's FTP Transfer function pushes files to. The
/// password is Rimix's own, made on first use and shown in the UI so it can
/// be typed into the camera; it is never logged.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SftpSettings {
    pub enabled: bool,
    pub port: u16,
    pub user: String,
    pub password: String,
}

impl Default for SftpSettings {
    fn default() -> Self {
        SftpSettings {
            enabled: false,
            port: 2222,
            user: "rimix".to_string(),
            password: String::new(),
        }
    }
}

impl std::fmt::Debug for SftpSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SftpSettings")
            .field("enabled", &self.enabled)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &"<hidden>")
            .finish()
    }
}

impl SftpSettings {
    /// Makes a password when there is none; true when it did.
    pub fn ensure_password(&mut self) -> bool {
        if !self.password.is_empty() {
            return false;
        }
        self.password = new_sftp_password();
        true
    }
}

/// Twelve characters from `a-z0-9`: easy to type on the camera.
pub fn new_sftp_password() -> String {
    use rand::RngExt;
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    (0..12)
        .map(|_| CHARS[rng.random_range(0..CHARS.len())] as char)
        .collect()
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
    pub wifi_join: WifiJoin,
    /// The webcam the camera appears as, when the user picked one.
    pub webcam_id: Option<String>,
    pub presets: Vec<Preset>,
    pub sftp: SftpSettings,
    /// Whether the connect screen looks for cameras over Bluetooth.
    pub bluetooth: bool,
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
            wifi_join: WifiJoin::default(),
            webcam_id: None,
            presets: Vec::new(),
            sftp: SftpSettings::default(),
            bluetooth: true,
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

/// Two camera addresses are the same when they differ only by the PTP/IP port.
fn same_address(a: &str, b: &str) -> bool {
    let bare = |s: &str| s.strip_suffix(":15740").unwrap_or(s).to_string();
    bare(a) == bare(b)
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

    /// Puts the camera first in the recent list, keeps at most
    /// [`MAX_RECENT`] and makes `ip` the last IP.
    ///
    /// Cameras are told apart by their network, because every camera on its
    /// own Wi-Fi Direct network has the same IP. A connect with an `ssid`
    /// replaces the entry with that ssid. A connect by address alone replaces
    /// an entry at that ip with no ssid, or one at that ip of the same model,
    /// which keeps its ssid.
    ///
    /// `ssh` is the Access Authentication (username, confirmed fingerprint)
    /// when the camera was reached through SSH; without it the replaced
    /// entry's login is kept. No password is ever stored.
    pub fn remember_camera(
        &mut self,
        ip: &str,
        model: &str,
        ssid: Option<&str>,
        ssh: Option<(&str, &str)>,
        now_millis: u64,
    ) {
        let older = self.recent_cameras.iter().position(|c| match ssid {
            Some(ssid) => c.ssid.as_deref() == Some(ssid),
            None => same_address(&c.ip, ip) && (c.ssid.is_none() || c.model == model),
        });
        let kept_ssid = match (ssid, older) {
            (Some(ssid), _) => Some(ssid.to_string()),
            (None, Some(i)) => self.recent_cameras[i].ssid.clone(),
            (None, None) => None,
        };
        let replaced = older.map(|i| self.recent_cameras.remove(i));
        let (ssh_user, ssh_fingerprint) = match (ssh, replaced) {
            (Some((user, fingerprint)), _) => {
                (Some(user.to_string()), Some(fingerprint.to_string()))
            }
            (None, Some(old)) => (old.ssh_user, old.ssh_fingerprint),
            (None, None) => (None, None),
        };
        self.recent_cameras.insert(
            0,
            RecentCamera {
                ip: ip.to_string(),
                model: model.to_string(),
                last_used: now_millis,
                ssid: kept_ssid,
                ssh_user,
                ssh_fingerprint,
            },
        );
        self.recent_cameras.truncate(MAX_RECENT);
        self.last_ip = Some(ip.to_string());
    }

    /// Drops the remembered camera with this `ssid`, or with no ssid at `ip`
    /// when `ssid` is `None`. If no camera is left at the last IP, the last
    /// IP becomes the newest camera still remembered, or none.
    pub fn forget_camera(&mut self, ssid: Option<&str>, ip: &str) {
        self.recent_cameras.retain(|c| match ssid {
            Some(ssid) => c.ssid.as_deref() != Some(ssid),
            None => c.ssid.is_some() || c.ip != ip,
        });
        let last_still_there = self
            .recent_cameras
            .iter()
            .any(|c| Some(c.ip.as_str()) == self.last_ip.as_deref());
        if !last_still_there {
            self.last_ip = self.recent_cameras.first().map(|c| c.ip.clone());
        }
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
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    // The file holds the SFTP receiver's password: owner only.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp)?;
    // A temp file left by a crash kept its old mode when reopened.
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    std::io::Write::write_all(&mut file, json.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn saved_settings_are_for_the_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        // An older file, and a temp file a crash left, both world readable.
        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let stale = path.with_extension("json.tmp");
        std::fs::write(&stale, "{}").unwrap();
        std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o644)).unwrap();
        save(&path, &AppSettings::default()).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{mode:o}");
        assert!(!stale.exists());
        assert_eq!(load(&path), AppSettings::default());
    }

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
        s.remember_camera("192.168.122.1", "ZV-E10", None, None, 1_000);
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
            s.remember_camera(&format!("10.0.0.{n}"), "ILCE-7M4", None, None, n);
        }
        s.remember_camera("10.0.0.5", "ILCE-7M4", None, None, 99);
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

    #[test]
    fn phase_1_settings_file_loads_with_new_defaults() {
        // Exactly what Phase 1 wrote: no ssid, wifiJoin, webcamId or presets.
        let text = r#"{"appearance":"Light","layout":"Dock","destinationMode":"FolderPerDate",
          "destinationBase":"/tmp/x","recentCameras":[{"ip":"192.168.122.1","model":"ZV-E10","lastUsed":5}],
          "lastIp":"192.168.122.1"}"#;
        let s: AppSettings = serde_json::from_str(text).unwrap();
        assert_eq!(s.wifi_join, WifiJoin::Automatic);
        assert_eq!(s.webcam_id, None);
        assert!(s.presets.is_empty());
        assert_eq!(s.recent_cameras[0].ssid, None);
        let back = serde_json::to_string(&s).unwrap();
        assert!(back.contains("\"wifiJoin\":\"Automatic\""), "{back}");
        assert!(back.contains("\"presets\":[]"), "{back}");
    }

    const WD: &str = "192.168.122.1";

    #[test]
    fn remember_camera_keeps_the_network_for_an_address_only_reconnect() {
        let mut s = AppSettings::default();
        s.remember_camera(WD, "ZV-E10", Some("DIRECT-abc1:ZV-E10"), None, 1);
        s.remember_camera(WD, "ZV-E10", None, None, 2); // same model, same ip, no ssid known
        assert_eq!(s.recent_cameras.len(), 1);
        assert_eq!(
            s.recent_cameras[0].ssid.as_deref(),
            Some("DIRECT-abc1:ZV-E10")
        );
        assert_eq!(s.recent_cameras[0].last_used, 2);
        s.remember_camera("10.0.0.9", "ZV-E10", Some("DIRECT-abc1:ZV-E10"), None, 3); // same network, router ip
        assert_eq!(s.recent_cameras.len(), 1);
        assert_eq!(s.recent_cameras[0].ip, "10.0.0.9");
    }

    #[test]
    fn an_address_with_and_without_the_port_is_one_camera() {
        let mut s = AppSettings::default();
        s.remember_camera(WD, "ZV-E10", None, None, 1);
        s.remember_camera("192.168.122.1:15740", "ZV-E10", None, None, 2);
        assert_eq!(s.recent_cameras.len(), 1);
        assert_eq!(s.recent_cameras[0].ip, "192.168.122.1:15740");
        assert_eq!(s.recent_cameras[0].last_used, 2);
    }

    #[test]
    fn two_cameras_at_the_wifi_direct_address_are_both_kept() {
        let mut s = AppSettings::default();
        s.remember_camera(WD, "ZV-E10", Some("DIRECT-abc1:ZV-E10"), None, 1);
        s.remember_camera(WD, "ILCE-7M4", Some("DIRECT-zz99:ILCE-7M4"), None, 2);
        let nets: Vec<_> = s
            .recent_cameras
            .iter()
            .map(|c| (c.model.as_str(), c.ssid.as_deref()))
            .collect();
        assert_eq!(
            nets,
            vec![
                ("ILCE-7M4", Some("DIRECT-zz99:ILCE-7M4")),
                ("ZV-E10", Some("DIRECT-abc1:ZV-E10")),
            ]
        );
    }

    #[test]
    fn an_address_only_connect_to_another_model_does_not_take_its_network() {
        let mut s = AppSettings::default();
        s.remember_camera(WD, "ZV-E10", Some("DIRECT-abc1:ZV-E10"), None, 1);
        s.remember_camera(WD, "ILCE-7M4", None, None, 2);
        assert_eq!(s.recent_cameras.len(), 2);
        assert_eq!(s.recent_cameras[0].model, "ILCE-7M4");
        assert_eq!(s.recent_cameras[0].ssid, None);
        assert_eq!(
            s.recent_cameras[1].ssid.as_deref(),
            Some("DIRECT-abc1:ZV-E10")
        );
        // Address-only entries still merge with each other.
        s.remember_camera(WD, "ILCE-7M4", None, None, 3);
        assert_eq!(s.recent_cameras.len(), 2);
    }

    #[test]
    fn forget_camera_removes_the_entry_with_that_network_or_address() {
        let mut s = AppSettings::default();
        s.remember_camera(WD, "ZV-E10", Some("DIRECT-abc1:ZV-E10"), None, 1);
        s.remember_camera(WD, "ILCE-7M4", Some("DIRECT-zz99:ILCE-7M4"), None, 2);
        s.remember_camera("10.0.0.7", "A7 IV", None, None, 3);
        s.forget_camera(Some("DIRECT-abc1:ZV-E10"), WD);
        let left: Vec<_> = s.recent_cameras.iter().map(|c| c.model.as_str()).collect();
        assert_eq!(left, vec!["A7 IV", "ILCE-7M4"]);
        assert_eq!(s.last_ip.as_deref(), Some("10.0.0.7"));
        s.forget_camera(None, "10.0.0.7");
        assert_eq!(s.recent_cameras.len(), 1);
        assert_eq!(s.last_ip.as_deref(), Some(WD));
        s.forget_camera(None, WD); // no ssid-less entry there: nothing goes
        assert_eq!(s.recent_cameras.len(), 1);
        s.forget_camera(Some("DIRECT-zz99:ILCE-7M4"), WD);
        assert!(s.recent_cameras.is_empty());
        assert_eq!(s.last_ip, None);
    }

    #[test]
    fn ssh_fields_default_to_none_and_round_trip() {
        let old: RecentCamera =
            serde_json::from_str(r#"{"ip":"1.2.3.4","model":"ZV-E10","lastUsed":1}"#).unwrap();
        assert_eq!(old.ssh_user, None);
        assert_eq!(old.ssh_fingerprint, None);
        let mut s = AppSettings::default();
        s.remember_camera(WD, "ILCE-7M4", None, Some(("cam", "SHA256:abc")), 1);
        let rc = &s.recent_cameras[0];
        assert_eq!(rc.ssh_user.as_deref(), Some("cam"));
        assert_eq!(rc.ssh_fingerprint.as_deref(), Some("SHA256:abc"));
        let text = serde_json::to_string(&s).unwrap();
        assert!(text.contains(r#""sshUser":"cam""#), "{text}");
        assert!(text.contains(r#""sshFingerprint":"SHA256:abc""#), "{text}");
        let back: AppSettings = serde_json::from_str(&text).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn a_connect_without_ssh_keeps_the_remembered_login() {
        let mut s = AppSettings::default();
        s.remember_camera(
            WD,
            "ILCE-7M4",
            Some("DIRECT-zz"),
            Some(("cam", "SHA256:abc")),
            1,
        );
        s.remember_camera(WD, "ILCE-7M4", Some("DIRECT-zz"), None, 2);
        assert_eq!(s.recent_cameras.len(), 1);
        assert_eq!(s.recent_cameras[0].ssh_user.as_deref(), Some("cam"));
        assert_eq!(
            s.recent_cameras[0].ssh_fingerprint.as_deref(),
            Some("SHA256:abc")
        );
        s.remember_camera(
            WD,
            "ILCE-7M4",
            Some("DIRECT-zz"),
            Some(("ops", "SHA256:new")),
            3,
        );
        assert_eq!(s.recent_cameras[0].ssh_user.as_deref(), Some("ops"));
        assert_eq!(
            s.recent_cameras[0].ssh_fingerprint.as_deref(),
            Some("SHA256:new")
        );
    }

    #[test]
    fn sftp_defaults_to_off_on_port_2222_as_rimix() {
        let s = AppSettings::default();
        assert!(!s.sftp.enabled);
        assert_eq!(s.sftp.port, 2222);
        assert_eq!(s.sftp.user, "rimix");
        assert_eq!(s.sftp.password, "");
    }

    #[test]
    fn bluetooth_is_on_by_default_and_round_trips() {
        assert!(AppSettings::default().bluetooth);
        let s: AppSettings = serde_json::from_str(r#"{"layout":"Ring"}"#).unwrap();
        assert!(s.bluetooth);
        let s: AppSettings = serde_json::from_str(r#"{"bluetooth":false}"#).unwrap();
        assert!(!s.bluetooth);
        assert!(serde_json::to_string(&s)
            .unwrap()
            .contains(r#""bluetooth":false"#));
    }

    #[test]
    fn a_settings_file_without_sftp_loads_with_its_defaults() {
        let text = r#"{"layout":"Ring","presets":[]}"#;
        let s: AppSettings = serde_json::from_str(text).unwrap();
        assert_eq!(s.sftp, SftpSettings::default());
        // A partial sftp object keeps what it has and defaults the rest.
        let s: AppSettings = serde_json::from_str(r#"{"sftp":{"enabled":true}}"#).unwrap();
        assert!(s.sftp.enabled);
        assert_eq!(s.sftp.port, 2222);
        assert_eq!(s.sftp.user, "rimix");
        let back = serde_json::to_string(&s).unwrap();
        assert!(
            back.contains(r#""sftp":{"enabled":true,"port":2222,"user":"rimix","password":"""#),
            "{back}"
        );
    }

    #[test]
    fn the_sftp_password_is_made_once_from_lowercase_letters_and_digits() {
        let mut s = SftpSettings::default();
        assert!(s.ensure_password());
        assert_eq!(s.password.len(), 12);
        assert!(s
            .password
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        let first = s.password.clone();
        assert!(!s.ensure_password());
        assert_eq!(s.password, first);
        assert_ne!(new_sftp_password(), new_sftp_password());
    }

    #[test]
    fn the_sftp_password_is_not_in_debug_output() {
        let s = SftpSettings {
            password: "abc123secret".into(),
            ..SftpSettings::default()
        };
        assert!(!format!("{s:?}").contains("abc123secret"));
    }

    #[test]
    fn preset_json_shape_is_stable() {
        let p = Preset {
            id: "p1".into(),
            name: "Talking head".into(),
            model: "ZV-E10".into(),
            slot: 1,
            values: vec![PresetValue {
                code: 0xD21E,
                raw: PtpValue::U32(800),
            }],
        };
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"id":"p1","name":"Talking head","model":"ZV-E10","slot":1,"values":[{"code":53790,"raw":{"U32":800}}]}"#
        );
    }
}
