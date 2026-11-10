//! The real radio through btleplug (CoreBluetooth on macOS, WinRT on
//! Windows). Compiled everywhere, exercised only against a real camera.

use std::time::Duration;

use btleplug::api::{
    Central, CentralState, CharPropFlags, Characteristic, Manager as _, Peripheral as _,
    ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use uuid::Uuid;

use crate::sony::{parse_sony_manufacturer_data, SONY_COMPANY_ID};
use crate::{BleAdapter, BleError, BoxFuture, Found};

/// How long connecting or discovering services may take.
const LINK_TIMEOUT: Duration = Duration::from_secs(15);
/// How long to listen when asked for a device the adapter has not seen yet.
const LOOKUP_SCAN: Duration = Duration::from_secs(4);

/// What macOS and Windows need before an app may use Bluetooth.
const PERMISSION: &str =
    "this app may not use Bluetooth; allow it in the system's privacy settings for Bluetooth";

fn map_err(e: btleplug::Error) -> BleError {
    match e {
        btleplug::Error::PermissionDenied => BleError::Unavailable(PERMISSION.to_string()),
        btleplug::Error::NoAdapterAvailable => {
            BleError::Unavailable("no Bluetooth adapter found".to_string())
        }
        btleplug::Error::DeviceNotFound | btleplug::Error::NoSuchCharacteristic => {
            BleError::NotFound
        }
        other => BleError::Io(other.to_string()),
    }
}

/// The first Bluetooth adapter of the machine.
pub struct BtleplugAdapter {
    central: Adapter,
}

impl BtleplugAdapter {
    /// Opens the adapter; `Unavailable` when there is none, it is off, or
    /// this process may not use it. Needs a tokio runtime.
    pub async fn new() -> Result<BtleplugAdapter, BleError> {
        let manager = Manager::new().await.map_err(unavailable)?;
        let central = manager
            .adapters()
            .await
            .map_err(unavailable)?
            .into_iter()
            .next()
            .ok_or_else(|| BleError::Unavailable("no Bluetooth adapter found".to_string()))?;
        if central.adapter_state().await.map_err(unavailable)? == CentralState::PoweredOff {
            return Err(BleError::Unavailable("Bluetooth is turned off".to_string()));
        }
        Ok(BtleplugAdapter { central })
    }

    async fn listen(&self, for_: Duration) -> Result<(), BleError> {
        // No service filter: cameras do not reliably advertise their services.
        self.central
            .start_scan(ScanFilter::default())
            .await
            .map_err(map_err)?;
        tokio::time::sleep(for_).await;
        self.central.stop_scan().await.map_err(map_err)
    }

    async fn known(&self, device: &str) -> Result<Option<Peripheral>, BleError> {
        let all = self.central.peripherals().await.map_err(map_err)?;
        Ok(all.into_iter().find(|p| p.id().to_string() == device))
    }

    /// The device, listening for it first when the adapter has not seen it.
    async fn peripheral(&self, device: &str) -> Result<Peripheral, BleError> {
        if let Some(p) = self.known(device).await? {
            return Ok(p);
        }
        self.listen(LOOKUP_SCAN).await?;
        self.known(device).await?.ok_or(BleError::NotFound)
    }

    /// The device's characteristic, connecting and discovering when needed.
    async fn ready(
        &self,
        device: &str,
        char: Uuid,
    ) -> Result<(Peripheral, Characteristic), BleError> {
        let p = self.peripheral(device).await?;
        if !p.is_connected().await.map_err(map_err)? {
            within(p.connect()).await?;
        }
        if p.services().is_empty() {
            within(p.discover_services()).await?;
        }
        let c = p
            .characteristics()
            .into_iter()
            .find(|c| c.uuid == char)
            .ok_or(BleError::NotFound)?;
        Ok((p, c))
    }
}

/// A connection-time failure means the radio cannot be used at all.
fn unavailable(e: btleplug::Error) -> BleError {
    match map_err(e) {
        BleError::Io(m) => BleError::Unavailable(m),
        other => other,
    }
}

async fn within(
    f: impl std::future::Future<Output = btleplug::Result<()>>,
) -> Result<(), BleError> {
    match tokio::time::timeout(LINK_TIMEOUT, f).await {
        Ok(r) => r.map_err(map_err),
        Err(_) => Err(BleError::Io(format!(
            "the camera did not answer within {} s",
            LINK_TIMEOUT.as_secs()
        ))),
    }
}

/// With response when the characteristic allows it, else without.
fn write_type(c: &Characteristic) -> WriteType {
    if c.properties.contains(CharPropFlags::WRITE) {
        WriteType::WithResponse
    } else {
        WriteType::WithoutResponse
    }
}

impl BleAdapter for BtleplugAdapter {
    fn scan(&self, for_: Duration) -> BoxFuture<'_, Result<Vec<Found>, BleError>> {
        Box::pin(async move {
            self.listen(for_).await?;
            let mut found = Vec::new();
            for p in self.central.peripherals().await.map_err(map_err)? {
                let Ok(Some(props)) = p.properties().await else {
                    continue;
                };
                let Some(data) = props.manufacturer_data.get(&SONY_COMPANY_ID) else {
                    continue;
                };
                let Some(mut advert) = parse_sony_manufacturer_data(SONY_COMPANY_ID, data) else {
                    continue;
                };
                let address = p.id().to_string();
                advert.name = props.local_name.or(props.advertisement_name);
                advert.address = address.clone();
                found.push(Found { address, advert });
            }
            Ok(found)
        })
    }

    fn read<'a>(&'a self, device: &'a str, char: Uuid) -> BoxFuture<'a, Result<Vec<u8>, BleError>> {
        Box::pin(async move {
            let (p, c) = self.ready(device, char).await?;
            p.read(&c).await.map_err(map_err)
        })
    }

    fn write<'a>(
        &'a self,
        device: &'a str,
        char: Uuid,
        data: &'a [u8],
    ) -> BoxFuture<'a, Result<(), BleError>> {
        Box::pin(async move {
            let (p, c) = self.ready(device, char).await?;
            p.write(&c, data, write_type(&c)).await.map_err(map_err)
        })
    }

    fn disconnect<'a>(&'a self, device: &'a str) -> BoxFuture<'a, Result<(), BleError>> {
        Box::pin(async move {
            if let Some(p) = self.known(device).await? {
                if p.is_connected().await.map_err(map_err)? {
                    p.disconnect().await.map_err(map_err)?;
                }
            }
            Ok(())
        })
    }
}
