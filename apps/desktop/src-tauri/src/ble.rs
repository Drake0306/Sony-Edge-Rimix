//! Bluetooth in the app: Sony cameras heard nearby, and their Wi-Fi details
//! read over Bluetooth so the app can join their network.
//!
//! The adapter is made on the first command (`RIMIX_BLE=fake` gives the
//! scripted ZV-E10, `off` none). When Bluetooth turns out to be unavailable
//! (no adapter, turned off, permission refused) that is remembered: later
//! commands fail at once with the same text, without asking the radio
//! again, until [`BleService::retry`]. Nothing read from the camera (the
//! password above all) is logged.

use std::sync::{Mutex as StdMutex, MutexGuard};
use std::time::Duration;

use rimix_ble::{BleAdapter, BleError};

/// The longest scan the webview may ask for.
pub const MAX_SCAN_SECS: u64 = 15;

/// What a camera's advertisement says, as the webview gets it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BleAdvert {
    /// The advertised name, e.g. `ZV-E10`.
    pub name: Option<String>,
    /// The two model code bytes as text, e.g. `E1`.
    pub model_code: String,
    /// Whether the camera is accepting a new remote right now.
    pub pairing_open: bool,
    pub address: String,
}

/// A camera heard during a scan, as the webview gets it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BleFound {
    pub address: String,
    pub advert: BleAdvert,
}

impl From<rimix_ble::Found> for BleFound {
    fn from(f: rimix_ble::Found) -> Self {
        BleFound {
            address: f.address,
            advert: BleAdvert {
                name: f.advert.name,
                model_code: f.advert.model_code,
                pairing_open: f.advert.pairing_open,
                address: f.advert.address,
            },
        }
    }
}

/// The camera's Wi-Fi network as `ble_handoff` gives it to the webview,
/// which joins it at once and never stores the password.
#[derive(Clone, PartialEq, Eq, serde::Serialize)]
pub struct BleCredentials {
    pub ssid: String,
    pub password: String,
}

impl std::fmt::Debug for BleCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BleCredentials")
            .field("ssid", &self.ssid)
            .field("password", &"[hidden]")
            .finish()
    }
}

/// How long a scan of `seconds` listens: 1 to [`MAX_SCAN_SECS`] seconds.
pub fn scan_duration(seconds: u64) -> Duration {
    Duration::from_secs(seconds.clamp(1, MAX_SCAN_SECS))
}

type Slot = Option<Box<dyn BleAdapter>>;

/// The adapter, made on first use, and why Bluetooth is unavailable when
/// it is.
#[derive(Default)]
pub struct BleService {
    adapter: tokio::sync::Mutex<Slot>,
    unavailable: StdMutex<Option<String>>,
}

fn lock<T>(m: &StdMutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl BleService {
    /// The adapter, made now if needed; the remembered `Unavailable` text
    /// without touching the radio when it is unavailable. Holding the guard
    /// keeps the radio to one command at a time.
    async fn adapter(&self) -> Result<tokio::sync::MutexGuard<'_, Slot>, String> {
        if let Some(why) = lock(&self.unavailable).clone() {
            return Err(BleError::Unavailable(why).to_string());
        }
        let mut slot = self.adapter.lock().await;
        if slot.is_none() {
            match rimix_ble::adapter_from_env().await {
                Ok(adapter) => *slot = Some(adapter),
                Err(e) => return Err(self.settle(&mut slot, e)),
            }
        }
        Ok(slot)
    }

    /// The text for `e`. An `Unavailable` is remembered, and the adapter
    /// dropped so [`BleService::retry`] starts afresh.
    fn settle(&self, slot: &mut Slot, e: BleError) -> String {
        if let BleError::Unavailable(why) = &e {
            tracing::warn!(error = %e, "Bluetooth is unavailable");
            *lock(&self.unavailable) = Some(why.clone());
            *slot = None;
        }
        e.to_string()
    }

    /// Runs `f` on the adapter.
    async fn run<T>(
        &self,
        f: impl for<'a> FnOnce(&'a dyn BleAdapter) -> rimix_ble::BoxFuture<'a, Result<T, BleError>>,
    ) -> Result<T, String> {
        let mut slot = self.adapter().await?;
        let result = match slot.as_deref() {
            Some(adapter) => f(adapter).await,
            None => return Err(BleError::NotFound.to_string()),
        };
        result.map_err(|e| self.settle(&mut slot, e))
    }

    /// The Sony cameras heard in `seconds` (1 to 15).
    pub async fn scan(&self, seconds: u64) -> Result<Vec<BleFound>, String> {
        let found = self.run(|a| a.scan(scan_duration(seconds))).await?;
        Ok(found.into_iter().map(BleFound::from).collect())
    }

    /// The Wi-Fi network of the camera at `address`.
    pub async fn handoff(&self, address: &str) -> Result<BleCredentials, String> {
        let address = address.to_string();
        let creds = self
            .run(move |a| Box::pin(async move { rimix_ble::handoff(a, &address).await }))
            .await?;
        Ok(BleCredentials {
            ssid: creds.ssid,
            password: creds.password,
        })
    }

    /// One photo through the camera's Bluetooth remote shutter.
    pub async fn shoot(&self, address: &str) -> Result<(), String> {
        let address = address.to_string();
        self.run(move |a| Box::pin(async move { rimix_ble::shoot(a, &address).await }))
            .await
    }

    /// Forgets that Bluetooth was unavailable; the next command makes the
    /// adapter again.
    pub async fn retry(&self) {
        *lock(&self.unavailable) = None;
        *self.adapter.lock().await = None;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Serialises the tests that set `RIMIX_BLE`.
    pub(crate) static BLE_ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    const OFF: &str = "Bluetooth unavailable: Bluetooth is off for this run";

    #[tokio::test]
    async fn the_fake_scan_lists_the_zv_e10() {
        let _guard = BLE_ENV.lock().await;
        std::env::set_var(rimix_ble::ENV, "fake");
        let found = BleService::default().scan(3).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].advert.name.as_deref(), Some("ZV-E10"));
        assert_eq!(found[0].advert.model_code, "E1");
        assert!(found[0].advert.pairing_open);
    }

    #[tokio::test]
    async fn a_handoff_returns_the_network_and_password() {
        let _guard = BLE_ENV.lock().await;
        std::env::set_var(rimix_ble::ENV, "fake");
        let ble = BleService::default();
        let address = ble.scan(3).await.unwrap()[0].address.clone();
        let creds = ble.handoff(&address).await.unwrap();
        assert_eq!(creds.ssid, "DIRECT-abc1:ZV-E10");
        assert_eq!(creds.password, "12345678");
        assert_eq!(
            ble.handoff("elsewhere").await.unwrap_err(),
            "no such Bluetooth device or characteristic"
        );
    }

    #[tokio::test]
    async fn a_shot_goes_to_the_camera() {
        let _guard = BLE_ENV.lock().await;
        std::env::set_var(rimix_ble::ENV, "fake");
        assert_eq!(BleService::default().shoot("fake-zv-e10").await, Ok(()));
    }

    #[tokio::test]
    async fn off_gives_the_unavailable_text() {
        let _guard = BLE_ENV.lock().await;
        std::env::set_var(rimix_ble::ENV, "off");
        let ble = BleService::default();
        assert_eq!(ble.scan(3).await.unwrap_err(), OFF);
        assert_eq!(ble.handoff("fake-zv-e10").await.unwrap_err(), OFF);
        assert_eq!(ble.shoot("fake-zv-e10").await.unwrap_err(), OFF);
    }

    #[tokio::test]
    async fn unavailable_is_remembered_until_retry() {
        let _guard = BLE_ENV.lock().await;
        std::env::set_var(rimix_ble::ENV, "off");
        let ble = BleService::default();
        assert_eq!(ble.scan(3).await.unwrap_err(), OFF);
        // The radio is not asked again: the fake would answer now.
        std::env::set_var(rimix_ble::ENV, "fake");
        assert_eq!(ble.scan(3).await.unwrap_err(), OFF);
        ble.retry().await;
        assert_eq!(ble.scan(3).await.unwrap().len(), 1);
    }

    #[test]
    fn scans_last_one_to_fifteen_seconds() {
        assert_eq!(scan_duration(0), Duration::from_secs(1));
        assert_eq!(scan_duration(3), Duration::from_secs(3));
        assert_eq!(scan_duration(99), Duration::from_secs(15));
    }

    #[test]
    fn found_json_shape_is_stable() {
        let found = BleFound {
            address: "fake-zv-e10".into(),
            advert: BleAdvert {
                name: Some("ZV-E10".into()),
                model_code: "E1".into(),
                pairing_open: true,
                address: "fake-zv-e10".into(),
            },
        };
        assert_eq!(
            serde_json::to_value(&found).unwrap(),
            serde_json::json!({
                "address": "fake-zv-e10",
                "advert": {
                    "name": "ZV-E10",
                    "model_code": "E1",
                    "pairing_open": true,
                    "address": "fake-zv-e10"
                }
            })
        );
    }

    #[test]
    fn the_password_is_not_in_debug_output() {
        let creds = BleCredentials {
            ssid: "DIRECT-abc1:ZV-E10".into(),
            password: "12345678".into(),
        };
        let shown = format!("{creds:?}");
        assert!(shown.contains("DIRECT-abc1"), "{shown}");
        assert!(!shown.contains("12345678"), "{shown}");
    }
}
