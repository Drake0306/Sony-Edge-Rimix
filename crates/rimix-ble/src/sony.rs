//! What Sony cameras say and accept over Bluetooth LE: the advertisement,
//! the GATT uuids, the Wi-Fi handoff and the remote shutter.

use std::time::Duration;

use uuid::Uuid;

use crate::{BleAdapter, BleError, SonyAdvertisement, WifiCredentials};

/// Sony's Bluetooth SIG company identifier.
pub const SONY_COMPANY_ID: u16 = 0x012D;

/// The Camera Control service (Wi-Fi details among others).
pub const SERVICE_CAMERA_CONTROL: Uuid = Uuid::from_u128(0x8000CC00_CC00_FFFF_FFFF_FFFFFFFFFFFF);
/// The camera's Wi-Fi network name (byte format undocumented).
pub const CHAR_SSID: Uuid = Uuid::from_u128(0x0000CC06_0000_1000_8000_00805F9B34FB);
/// The camera's Wi-Fi password (byte format undocumented).
pub const CHAR_PASSWORD: Uuid = Uuid::from_u128(0x0000CC07_0000_1000_8000_00805F9B34FB);
/// The Remote Control service.
pub const SERVICE_REMOTE: Uuid = Uuid::from_u128(0x8000FF00_FF00_FFFF_FFFF_FFFFFFFFFFFF);
/// Where remote commands such as the shutter are written.
pub const CHAR_REMOTE_COMMAND: Uuid = Uuid::from_u128(0x0000FF01_0000_1000_8000_00805F9B34FB);
/// Where the camera reports focus and shutter state.
pub const CHAR_REMOTE_NOTIFY: Uuid = Uuid::from_u128(0x0000FF02_0000_1000_8000_00805F9B34FB);

/// One photo: half press, full press, full release, half release.
pub const SHUTTER_SEQUENCE: [[u8; 2]; 4] = [[0x01, 0x07], [0x01, 0x09], [0x01, 0x08], [0x01, 0x06]];
/// The pause between the shutter writes.
pub const SHUTTER_GAP: Duration = Duration::from_millis(100);

/// The device type bytes of a camera.
const CAMERA: [u8; 2] = [0x03, 0x00];
/// The tag that precedes the pairing status byte.
const TAG_PAIRING: u8 = 0x22;
/// The pairing status of a camera that is accepting a new remote.
const PAIRING_OPEN: u8 = 0xEF;

/// A Sony camera's advertisement from its manufacturer data, the company id
/// apart (btleplug hands it over separately): `03 00` camera, the protocol
/// version (`64 00` in the research's example, 100 little-endian), two
/// model code bytes, then tagged fields where tag `0x22`
/// carries the pairing status (`0xEF` open, `0xAF` not). `name` and
/// `address` are left for the adapter to fill in.
pub fn parse_sony_manufacturer_data(company_id: u16, data: &[u8]) -> Option<SonyAdvertisement> {
    if company_id != SONY_COMPANY_ID || data.len() < 7 || data[0..2] != CAMERA {
        return None;
    }
    let model_code = String::from_utf8_lossy(&data[4..6]).into_owned();
    let pairing_open = data[6..]
        .windows(2)
        .find(|pair| pair[0] == TAG_PAIRING)
        .is_some_and(|pair| pair[1] == PAIRING_OPEN);
    Some(SonyAdvertisement {
        name: None,
        model_code,
        pairing_open,
        address: String::new(),
    })
}

/// Bytes as lowercase hex without separators.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A characteristic's text: trailing NULs dropped, invalid UTF-8 replaced.
fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Reads the camera's Wi-Fi network name and password. Only a name starting
/// with `DIRECT-` is accepted, since the byte format is undocumented;
/// anything else fails with the name's bytes in hex (never the password's)
/// so the owner can report it.
pub async fn handoff(adapter: &dyn BleAdapter, device: &str) -> Result<WifiCredentials, BleError> {
    let result = read_credentials(adapter, device).await;
    let _ = adapter.disconnect(device).await;
    result
}

async fn read_credentials(
    adapter: &dyn BleAdapter,
    device: &str,
) -> Result<WifiCredentials, BleError> {
    let ssid_bytes = adapter.read(device, CHAR_SSID).await?;
    let password_bytes = adapter.read(device, CHAR_PASSWORD).await?;
    let ssid = text(&ssid_bytes);
    if !ssid.starts_with("DIRECT-") {
        return Err(BleError::Io(format!(
            "unrecognised credential format: {}",
            hex(&ssid_bytes)
        )));
    }
    Ok(WifiCredentials {
        ssid,
        password: text(&password_bytes),
    })
}

/// Takes one photo by writing [`SHUTTER_SEQUENCE`] to the remote command
/// characteristic, [`SHUTTER_GAP`] apart.
pub async fn shoot(adapter: &dyn BleAdapter, device: &str) -> Result<(), BleError> {
    let result = press(adapter, device).await;
    let _ = adapter.disconnect(device).await;
    result
}

async fn press(adapter: &dyn BleAdapter, device: &str) -> Result<(), BleError> {
    for (i, frame) in SHUTTER_SEQUENCE.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(SHUTTER_GAP).await;
        }
        if let Err(e) = adapter.write(device, CHAR_REMOTE_COMMAND, frame).await {
            release(adapter, device, i).await;
            return Err(e);
        }
    }
    Ok(())
}

/// After the write of frame `failed` failed, tries once to let go of
/// whatever was pressed so the shutter is not left held down.
async fn release(adapter: &dyn BleAdapter, device: &str, failed: usize) {
    // Frames 0 (half down) and 1 (full down) press; 2 and 3 release.
    let pending: &[[u8; 2]] = match failed {
        0 => &[],
        1 | 2 => &SHUTTER_SEQUENCE[2..],
        _ => &SHUTTER_SEQUENCE[3..],
    };
    for frame in pending {
        let _ = adapter.write(device, CHAR_REMOTE_COMMAND, frame).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: [u8; 11] = [
        0x2D, 0x01, 0x03, 0x00, 0x64, 0x00, 0x45, 0x31, 0x22, 0xEF, 0x00,
    ];

    #[test]
    fn the_research_example_is_a_camera_with_pairing_open() {
        // btleplug hands over the company id apart from the data.
        let advert = parse_sony_manufacturer_data(0x012D, &EXAMPLE[2..]).unwrap();
        assert_eq!(advert.model_code, "E1");
        assert!(advert.pairing_open);
        assert_eq!(advert.name, None);
    }

    #[test]
    fn status_af_means_pairing_closed() {
        let mut data = EXAMPLE[2..].to_vec();
        data[7] = 0xAF;
        let advert = parse_sony_manufacturer_data(0x012D, &data).unwrap();
        assert!(!advert.pairing_open);
    }

    #[test]
    fn another_company_is_not_a_sony_camera() {
        assert_eq!(parse_sony_manufacturer_data(0x004C, &EXAMPLE[2..]), None);
    }

    #[test]
    fn short_data_or_another_device_type_is_not_a_camera() {
        assert_eq!(
            parse_sony_manufacturer_data(0x012D, &[0x03, 0x00, 0x64]),
            None
        );
        let mut data = EXAMPLE[2..].to_vec();
        data[0] = 0x01;
        assert_eq!(parse_sony_manufacturer_data(0x012D, &data), None);
    }

    #[test]
    fn the_shutter_is_half_full_full_up_half_up() {
        assert_eq!(
            SHUTTER_SEQUENCE,
            [[0x01, 0x07], [0x01, 0x09], [0x01, 0x08], [0x01, 0x06]]
        );
        assert_eq!(SHUTTER_GAP, Duration::from_millis(100));
    }

    #[test]
    fn the_uuids_are_sonys() {
        assert_eq!(
            SERVICE_CAMERA_CONTROL.to_string(),
            "8000cc00-cc00-ffff-ffff-ffffffffffff"
        );
        assert_eq!(
            CHAR_SSID.to_string(),
            "0000cc06-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            CHAR_REMOTE_COMMAND.to_string(),
            "0000ff01-0000-1000-8000-00805f9b34fb"
        );
    }
}
