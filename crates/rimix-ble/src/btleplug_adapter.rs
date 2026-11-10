//! The real radio through btleplug (CoreBluetooth on macOS, WinRT on
//! Windows). Compiled everywhere, exercised only against a real camera.

use std::time::Duration;

use btleplug::api::{
    Central, CentralState, CharPropFlags, Characteristic, Manager as _, Peripheral as _,
    ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use uuid::Uuid;

use crate::sony::{parse_sony_manufacturer_data, pick_index, SONY_COMPANY_ID};
use crate::{BleAdapter, BleError, BoxFuture, Found};

/// How long connecting or discovering services may take.
const LINK_TIMEOUT: Duration = Duration::from_secs(15);
/// How long any other call on the radio or the camera may take (read,
/// write, disconnect, scan start and stop, listing what was heard).
const OP_TIMEOUT: Duration = Duration::from_secs(5);
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
        let manager = setup(Manager::new()).await?;
        let central = setup(manager.adapters())
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| BleError::Unavailable("no Bluetooth adapter found".to_string()))?;
        if setup(central.adapter_state()).await? == CentralState::PoweredOff {
            return Err(BleError::Unavailable("Bluetooth is turned off".to_string()));
        }
        Ok(BtleplugAdapter { central })
    }

    async fn listen(&self, for_: Duration) -> Result<(), BleError> {
        // No service filter: cameras do not reliably advertise their services.
        within(
            OP_TIMEOUT,
            "start scanning",
            self.central.start_scan(ScanFilter::default()),
        )
        .await?;
        tokio::time::sleep(for_).await;
        within(OP_TIMEOUT, "stop scanning", self.central.stop_scan()).await
    }

    async fn known(&self, device: &str) -> Result<Option<Peripheral>, BleError> {
        let all = within(OP_TIMEOUT, "list devices", self.central.peripherals()).await?;
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
        if !within(OP_TIMEOUT, "report its link", p.is_connected()).await? {
            within(LINK_TIMEOUT, "connect", p.connect()).await?;
        }
        if p.services().is_empty() {
            within(LINK_TIMEOUT, "list its services", p.discover_services()).await?;
        }
        let all: Vec<Characteristic> = p.characteristics().into_iter().collect();
        let candidates: Vec<(Uuid, Option<Uuid>)> =
            all.iter().map(|c| (c.uuid, Some(c.service_uuid))).collect();
        let i = pick_index(char, &candidates).ok_or(BleError::NotFound)?;
        let c = all[i].clone();
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

/// Runs `f` for at most `limit`; past that, `Io("timed out after … waiting
/// to <what>")`.
async fn within<T>(
    limit: Duration,
    what: &str,
    f: impl std::future::Future<Output = btleplug::Result<T>>,
) -> Result<T, BleError> {
    match tokio::time::timeout(limit, f).await {
        Ok(r) => r.map_err(map_err),
        Err(_) => Err(BleError::Io(format!(
            "timed out after {} ms waiting to {what}",
            limit.as_millis()
        ))),
    }
}

/// Opening the adapter, bounded: any failure means it cannot be used.
async fn setup<T>(
    f: impl std::future::Future<Output = btleplug::Result<T>>,
) -> Result<T, BleError> {
    match tokio::time::timeout(OP_TIMEOUT, f).await {
        Ok(r) => r.map_err(unavailable),
        Err(_) => Err(BleError::Unavailable(format!(
            "the Bluetooth adapter did not answer within {} s",
            OP_TIMEOUT.as_secs()
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
            for p in within(OP_TIMEOUT, "list devices", self.central.peripherals()).await? {
                let Ok(Some(props)) = within(OP_TIMEOUT, "read an advert", p.properties()).await
                else {
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
        self.read_within(device, char, OP_TIMEOUT)
    }

    fn read_within<'a>(
        &'a self,
        device: &'a str,
        char: Uuid,
        limit: Duration,
    ) -> BoxFuture<'a, Result<Vec<u8>, BleError>> {
        Box::pin(async move {
            let (p, c) = self.ready(device, char).await?;
            within(limit, "read from the camera", p.read(&c)).await
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
            within(
                OP_TIMEOUT,
                "write to the camera",
                p.write(&c, data, write_type(&c)),
            )
            .await
        })
    }

    fn disconnect<'a>(&'a self, device: &'a str) -> BoxFuture<'a, Result<(), BleError>> {
        Box::pin(async move {
            if let Some(p) = self.known(device).await? {
                if within(OP_TIMEOUT, "report its link", p.is_connected()).await? {
                    within(OP_TIMEOUT, "disconnect", p.disconnect()).await?;
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_camera_that_never_answers_times_out() {
        let pending = std::future::pending::<btleplug::Result<Vec<u8>>>();
        let err = within(Duration::from_millis(20), "read from the camera", pending)
            .await
            .unwrap_err();
        assert_eq!(
            err,
            BleError::Io("timed out after 20 ms waiting to read from the camera".into())
        );
    }

    #[tokio::test]
    async fn an_answer_in_time_is_returned() {
        let ready = async { Ok(vec![1u8, 2]) };
        assert_eq!(
            within(Duration::from_millis(20), "read", ready).await,
            Ok(vec![1, 2])
        );
    }

    #[tokio::test]
    async fn a_failure_in_time_is_mapped() {
        let failed = async { Err::<(), _>(btleplug::Error::DeviceNotFound) };
        assert_eq!(
            within(Duration::from_millis(20), "read", failed).await,
            Err(BleError::NotFound)
        );
    }
}
