//! Finding Sony cameras over Bluetooth LE and talking to them. The radio
//! lives behind [`BleAdapter`]: [`system_adapter`] is the real one
//! (btleplug: CoreBluetooth on macOS, WinRT on Windows) and [`FakeAdapter`]
//! a scripted one for tests and for `RIMIX_BLE=fake` runs. Nothing read from
//! the camera (the Wi-Fi password above all) is logged here.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;

use uuid::Uuid;

mod btleplug_adapter;
pub mod sony;

pub use btleplug_adapter::BtleplugAdapter;
pub use sony::{handoff, parse_sony_manufacturer_data, shoot};

/// A boxed future, so [`BleAdapter`] can be used as `dyn BleAdapter`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What a Sony camera's advertisement says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SonyAdvertisement {
    /// The advertised local name, e.g. `ZV-E10`.
    pub name: Option<String>,
    /// The two model code bytes as text, e.g. `E1`.
    pub model_code: String,
    /// Whether the camera is accepting a new remote right now.
    pub pairing_open: bool,
    /// The adapter's id for the device.
    pub address: String,
}

/// A camera seen during a scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub address: String,
    pub advert: SonyAdvertisement,
}

/// The camera's Wi-Fi network, read over Bluetooth.
#[derive(Clone, PartialEq, Eq)]
pub struct WifiCredentials {
    pub ssid: String,
    pub password: String,
}

impl fmt::Debug for WifiCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WifiCredentials")
            .field("ssid", &self.ssid)
            .field("password", &"[hidden]")
            .finish()
    }
}

/// What can go wrong over Bluetooth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BleError {
    /// No adapter, Bluetooth off, or no permission to use it.
    Unavailable(String),
    /// No such device or characteristic.
    NotFound,
    /// The link or the camera failed.
    Io(String),
}

impl fmt::Display for BleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BleError::Unavailable(m) => write!(f, "Bluetooth is unavailable: {m}"),
            BleError::NotFound => f.write_str("no such Bluetooth device or characteristic"),
            BleError::Io(m) => write!(f, "Bluetooth failed: {m}"),
        }
    }
}

impl std::error::Error for BleError {}

/// The radio. `device` is a [`Found::address`].
pub trait BleAdapter: Send + Sync {
    /// Listens for `for_` and returns the Sony cameras heard.
    fn scan(&self, for_: Duration) -> BoxFuture<'_, Result<Vec<Found>, BleError>>;
    /// Reads a characteristic, connecting first when needed.
    fn read<'a>(&'a self, device: &'a str, char: Uuid) -> BoxFuture<'a, Result<Vec<u8>, BleError>>;
    /// Writes a characteristic, connecting first when needed.
    fn write<'a>(
        &'a self,
        device: &'a str,
        char: Uuid,
        data: &'a [u8],
    ) -> BoxFuture<'a, Result<(), BleError>>;
    /// Drops the connection to `device`, if there is one.
    fn disconnect<'a>(&'a self, device: &'a str) -> BoxFuture<'a, Result<(), BleError>> {
        let _ = device;
        Box::pin(async { Ok(()) })
    }
}

/// The env flag that picks the adapter.
pub const ENV: &str = "RIMIX_BLE";

/// The machine's Bluetooth adapter, or `Unavailable` when there is none,
/// it is off, or this process may not use it. Needs a tokio runtime.
pub async fn system_adapter() -> Result<Box<dyn BleAdapter>, BleError> {
    Ok(Box::new(BtleplugAdapter::new().await?))
}

/// `RIMIX_BLE=fake`: [`FakeAdapter::zv_e10`]; `RIMIX_BLE=off`: always
/// `Unavailable`; otherwise [`system_adapter`].
pub async fn adapter_from_env() -> Result<Box<dyn BleAdapter>, BleError> {
    match std::env::var(ENV).as_deref() {
        Ok("fake") => Ok(Box::new(FakeAdapter::zv_e10())),
        Ok("off") => Err(BleError::Unavailable(
            "Bluetooth is off for this run".to_string(),
        )),
        _ => system_adapter().await,
    }
}

/// A scripted adapter: devices to find, characteristic values to read, and
/// a record of every write.
#[derive(Default)]
pub struct FakeAdapter {
    devices: Vec<Found>,
    values: HashMap<(String, Uuid), Vec<u8>>,
    writes: Mutex<Vec<(String, Uuid, Vec<u8>)>>,
    fail_write: Option<usize>,
    attempts: Mutex<usize>,
}

impl FakeAdapter {
    pub fn new() -> FakeAdapter {
        FakeAdapter::default()
    }

    /// The camera `RIMIX_BLE=fake` finds: a ZV-E10 with pairing open whose
    /// network is `DIRECT-abc1:ZV-E10` / `12345678`.
    pub fn zv_e10() -> FakeAdapter {
        let address = "fake-zv-e10";
        FakeAdapter::new()
            .device(
                address,
                SonyAdvertisement {
                    name: Some("ZV-E10".to_string()),
                    model_code: "E1".to_string(),
                    pairing_open: true,
                    address: address.to_string(),
                },
            )
            .characteristic(address, sony::CHAR_SSID, b"DIRECT-abc1:ZV-E10")
            .characteristic(address, sony::CHAR_PASSWORD, b"12345678")
    }

    /// A device the scan finds.
    pub fn device(mut self, address: &str, advert: SonyAdvertisement) -> FakeAdapter {
        self.devices.push(Found {
            address: address.to_string(),
            advert,
        });
        self
    }

    /// A value `read` returns.
    pub fn characteristic(mut self, address: &str, char: Uuid, bytes: &[u8]) -> FakeAdapter {
        self.values
            .insert((address.to_string(), char), bytes.to_vec());
        self
    }

    /// Makes write attempt number `n` (counting from 0) fail.
    pub fn fail_write(mut self, n: usize) -> FakeAdapter {
        self.fail_write = Some(n);
        self
    }

    /// Every successful write so far: device, characteristic, bytes.
    pub fn writes(&self) -> Vec<(String, Uuid, Vec<u8>)> {
        self.writes.lock().unwrap().clone()
    }

    fn knows(&self, device: &str) -> bool {
        self.devices.iter().any(|d| d.address == device)
    }
}

impl BleAdapter for FakeAdapter {
    fn scan(&self, _for: Duration) -> BoxFuture<'_, Result<Vec<Found>, BleError>> {
        Box::pin(async move { Ok(self.devices.clone()) })
    }

    fn read<'a>(&'a self, device: &'a str, char: Uuid) -> BoxFuture<'a, Result<Vec<u8>, BleError>> {
        Box::pin(async move {
            self.values
                .get(&(device.to_string(), char))
                .cloned()
                .ok_or(BleError::NotFound)
        })
    }

    fn write<'a>(
        &'a self,
        device: &'a str,
        char: Uuid,
        data: &'a [u8],
    ) -> BoxFuture<'a, Result<(), BleError>> {
        Box::pin(async move {
            if !self.knows(device) {
                return Err(BleError::NotFound);
            }
            let attempt = {
                let mut attempts = self.attempts.lock().unwrap();
                *attempts += 1;
                *attempts - 1
            };
            if self.fail_write == Some(attempt) {
                return Err(BleError::Io("scripted write failure".to_string()));
            }
            self.writes
                .lock()
                .unwrap()
                .push((device.to_string(), char, data.to_vec()));
            Ok(())
        })
    }
}
