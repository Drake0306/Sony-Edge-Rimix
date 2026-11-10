//! Subcommand bodies. Each connects, does its work and disconnects.

use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use rimix_ble::{BleAdapter, BleError, Found};
use rimix_wifi::{SystemRunner, Wifi, WifiError};
use sony_ptpip::capture::{Destination, DestinationMode};
use sony_ptpip::ptp::PtpValue;
use sony_ptpip::{
    default_host_name, load_or_create_guid, Camera, CameraConfig, CameraEvent, CardObject,
    PropertySummary, SshConfig,
};
use tokio::sync::broadcast::{error::RecvError, Receiver};
use tokio::time::{timeout_at, Instant};

use crate::format::{card_table, hex_code, props_table, size_text, sort_card, DOCK_PROPS};

/// How long shoot and liveview wait for the camera.
const WAIT: Duration = Duration::from_secs(20);
/// After a file of a shot is saved, how long shoot waits for the camera to
/// start sending another (RAW+JPEG gives two files per shot).
const MORE_FILES_WAIT: Duration = Duration::from_secs(2);

/// Failures that are the CLI's own rather than the camera's.
#[derive(Debug)]
pub enum CliError {
    /// Bad arguments that clap cannot check (exit 2).
    Usage(String),
    /// A local file or a capture failed (exit 5).
    Io(String),
    /// The camera stopped accepting this computer while watching: its SSH
    /// login was refused or its fingerprint changed (exit 4).
    Refused(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Usage(m) | CliError::Io(m) | CliError::Refused(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for CliError {}

fn io_error(what: &str, e: std::io::Error) -> anyhow::Error {
    anyhow::Error::new(CliError::Io(format!("{what}: {e}")))
}

/// How to reach the camera, from the global options.
pub struct Target {
    pub addr: SocketAddr,
    pub guid_file: PathBuf,
    /// The camera's Access Authentication login, when given.
    pub ssh: Option<SshConfig>,
}

async fn open(
    target: &Target,
    name: Option<String>,
    out: Option<&Path>,
    auto_receive: bool,
    reconnect: bool,
) -> Result<Camera> {
    let guid = load_or_create_guid(&target.guid_file)
        .map_err(|e| anyhow::anyhow!(CliError::Io(format!("guid file: {e}"))))?;
    let destination = Destination {
        base: out.map(Path::to_path_buf).unwrap_or_else(|| ".".into()),
        mode: DestinationMode::CustomFolder,
        recovery_dir: None,
    };
    let camera = Camera::connect(CameraConfig {
        addr: target.addr,
        guid,
        host_name: name.unwrap_or_else(default_host_name),
        destination,
        auto_receive,
        reconnect,
        before_reconnect: None,
        ssh: target.ssh.clone(),
    })
    .await
    .with_context(|| format!("cannot connect to {}", target.addr))?;
    Ok(camera)
}

pub async fn discover(timeout: Duration, ssdp_target: Option<SocketAddr>) -> Result<()> {
    let cameras = match ssdp_target {
        None => sony_ptpip::discover(timeout).await?,
        Some(addr) => {
            let mut found = Vec::new();
            for location in sony_ptpip::discovery::ssdp_search(timeout, Some(addr)).await? {
                if let Ok(camera) = sony_ptpip::discovery::fetch_description(&location).await {
                    found.push(camera);
                }
            }
            found
        }
    };
    if cameras.is_empty() {
        println!("No cameras found");
    }
    for c in cameras {
        println!(
            "{} ({}) at {}:{}",
            c.friendly_name, c.model, c.ip, c.ptp_port
        );
    }
    Ok(())
}

pub async fn connect(target: &Target, name: Option<String>) -> Result<()> {
    let camera = open(target, name, None, false, false).await?;
    let state = camera.state();
    println!("Model: {}", state.model);
    println!("Firmware: {}", state.firmware);
    match state.battery {
        Some(n) => println!("Battery: {n}%"),
        None => println!("Battery: unknown"),
    }
    camera.disconnect().await;
    Ok(())
}

pub async fn props(target: &Target, all: bool) -> Result<()> {
    let camera = open(target, None, None, false, false).await?;
    let state = camera.state();
    camera.disconnect().await;
    let shown: Vec<&PropertySummary> = if all {
        state.properties.iter().collect()
    } else {
        DOCK_PROPS
            .iter()
            .filter_map(|code| state.properties.iter().find(|p| p.code == *code))
            .collect()
    };
    print!("{}", props_table(&shown));
    Ok(())
}

/// Parses decimal or `0x` hex into the same integer type as `like`.
fn parse_value(text: &str, like: &PtpValue) -> Result<PtpValue> {
    let usage = |m: String| anyhow::Error::new(CliError::Usage(m));
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let magnitude = match digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        Some(hex) => i128::from_str_radix(hex, 16),
        None => digits.parse::<i128>(),
    }
    .map_err(|_| usage(format!("{text:?} is not a number")))?;
    let n = if negative { -magnitude } else { magnitude };
    let range = |what: &str| usage(format!("{text} does not fit {what}"));
    Ok(match like {
        PtpValue::I8(_) => PtpValue::I8(i8::try_from(n).map_err(|_| range("a signed byte"))?),
        PtpValue::U8(_) => PtpValue::U8(u8::try_from(n).map_err(|_| range("a byte"))?),
        PtpValue::I16(_) => PtpValue::I16(i16::try_from(n).map_err(|_| range("16 bits signed"))?),
        PtpValue::U16(_) => PtpValue::U16(u16::try_from(n).map_err(|_| range("16 bits"))?),
        PtpValue::I32(_) => PtpValue::I32(i32::try_from(n).map_err(|_| range("32 bits signed"))?),
        PtpValue::U32(_) => PtpValue::U32(u32::try_from(n).map_err(|_| range("32 bits"))?),
        PtpValue::I64(_) => PtpValue::I64(i64::try_from(n).map_err(|_| range("64 bits signed"))?),
        PtpValue::U64(_) => PtpValue::U64(u64::try_from(n).map_err(|_| range("64 bits"))?),
        PtpValue::Str(_) | PtpValue::Array(_) => {
            return Err(usage("only numeric properties can be set".into()))
        }
    })
}

fn find(camera: &Camera, code: u16) -> Result<PropertySummary> {
    camera
        .state()
        .properties
        .into_iter()
        .find(|p| p.code == code)
        .ok_or_else(|| anyhow::anyhow!("the camera has no property {}", hex_code(code)))
}

fn report_set(camera: &Camera, code: u16) -> Result<()> {
    let prop = find(camera, code)?;
    println!("Set {} to {}", hex_code(code), prop.label);
    Ok(())
}

pub async fn set(target: &Target, code: u16, value: &str) -> Result<()> {
    let camera = open(target, None, None, false, false).await?;
    let result = async {
        let value = parse_value(value, &find(&camera, code)?.raw)?;
        camera.set(code, value).await?;
        report_set(&camera, code)
    }
    .await;
    camera.disconnect().await;
    result
}

pub async fn step(target: &Target, code: u16, delta: i32) -> Result<()> {
    let camera = open(target, None, None, false, false).await?;
    let result = async {
        camera.step(code, delta).await?;
        report_set(&camera, code)
    }
    .await;
    camera.disconnect().await;
    result
}

/// Next event, or `None` when the deadline passes; lagging is skipped.
async fn next_event(rx: &mut Receiver<CameraEvent>, deadline: Instant) -> Option<CameraEvent> {
    loop {
        match timeout_at(deadline, rx.recv()).await {
            Err(_) => return None,
            Ok(Ok(event)) => return Some(event),
            Ok(Err(RecvError::Lagged(_))) => {}
            Ok(Err(RecvError::Closed)) => return None,
        }
    }
}

pub async fn shoot(target: &Target, out: &Path) -> Result<()> {
    let camera = open(target, None, Some(out), true, false).await?;
    let mut rx = camera.subscribe();
    let result = async {
        camera.shoot().await?;
        // Every file of the shot is saved and printed: after each one, wait
        // a little for the camera to start on the next.
        let mut until = Instant::now() + WAIT;
        let mut saved_any = false;
        loop {
            match next_event(&mut rx, until).await {
                Some(CameraEvent::CaptureStarted { .. }) => until = Instant::now() + WAIT,
                Some(CameraEvent::CaptureSaved(saved)) => {
                    println!("Saved {}", saved.path.display());
                    saved_any = true;
                    until = Instant::now() + MORE_FILES_WAIT;
                }
                Some(CameraEvent::CaptureSkipped { .. }) => {
                    until = Instant::now() + MORE_FILES_WAIT;
                }
                Some(CameraEvent::CaptureFailed { filename, error }) => {
                    return Err(anyhow::Error::new(CliError::Io(format!(
                        "capture of {filename} failed: {error}"
                    ))))
                }
                Some(_) => {}
                None if saved_any => return Ok(()),
                None => {
                    return Err(anyhow::Error::new(CliError::Io(
                        "no picture arrived within 20 seconds".into(),
                    )))
                }
            }
        }
    }
    .await;
    camera.disconnect().await;
    result
}

pub async fn record(target: &Target) -> Result<()> {
    let camera = open(target, None, None, false, false).await?;
    let result = async {
        camera.toggle_record().await?;
        // The camera reports the new state through a property change event.
        let mut rx = camera.subscribe();
        let before = camera.state().recording;
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut now = camera.state().recording;
        while now == before {
            match next_event(&mut rx, deadline).await {
                Some(CameraEvent::StateChanged(s)) => now = s.recording,
                Some(_) => {}
                None => break,
            }
        }
        println!(
            "{}",
            if now {
                "Recording started"
            } else {
                "Recording stopped"
            }
        );
        Ok(())
    }
    .await;
    camera.disconnect().await;
    result
}

pub async fn liveview(target: &Target, frames: u32, out: &Path) -> Result<()> {
    std::fs::create_dir_all(out).map_err(|e| io_error("cannot create the output folder", e))?;
    let camera = open(target, None, None, false, false).await?;
    let mut rx = camera.subscribe();
    let result = async {
        let deadline = Instant::now() + WAIT;
        let mut last: Vec<u8> = Vec::new();
        let mut saved = 0;
        while saved < frames {
            match next_event(&mut rx, deadline).await {
                Some(CameraEvent::Frame) => {
                    let Some(frame) = camera.latest_frame() else {
                        continue;
                    };
                    if frame.jpeg == last {
                        continue;
                    }
                    saved += 1;
                    let path = out.join(format!("frame-{saved:04}.jpg"));
                    std::fs::write(&path, &frame.jpeg)
                        .map_err(|e| io_error(&format!("cannot write {}", path.display()), e))?;
                    last = frame.jpeg.clone();
                }
                Some(_) => {}
                None => {
                    return Err(anyhow::Error::new(CliError::Io(format!(
                        "only {saved} of {frames} frames arrived within 20 seconds"
                    ))))
                }
            }
        }
        Ok(())
    }
    .await;
    camera.disconnect().await;
    result
}

/// `rimix card list`: the card's files, newest first.
pub async fn card_list(target: &Target) -> Result<()> {
    let camera = open(target, None, None, false, false).await?;
    let card = camera.card_list().await;
    camera.disconnect().await;
    let mut card = card?;
    sort_card(&mut card);
    print!("{}", card_table(&card));
    Ok(())
}

/// The card file `wanted` names: its handle (decimal or `0x` hex) or its
/// name, compared without case.
pub fn resolve<'a>(card: &'a [CardObject], wanted: &str) -> Result<&'a CardObject> {
    let usage = |m: String| anyhow::Error::new(CliError::Usage(m));
    let handle = match wanted
        .strip_prefix("0x")
        .or_else(|| wanted.strip_prefix("0X"))
    {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => wanted.parse::<u32>().ok(),
    };
    if let Some(obj) = handle.and_then(|h| card.iter().find(|o| o.handle == h)) {
        return Ok(obj);
    }
    let named: Vec<&CardObject> = card
        .iter()
        .filter(|o| o.filename.eq_ignore_ascii_case(wanted))
        .collect();
    match named.as_slice() {
        [one] => Ok(one),
        [] => Err(usage(format!("no file named {wanted} on the card"))),
        many => Err(usage(format!(
            "{} files are named {wanted}; give the handle instead",
            many.len()
        ))),
    }
}

/// One progress line: `DSC00010.JPG: 1.0 MB of 2.0 MB (50%)`.
pub fn progress_text(filename: &str, received: u64, total: u64) -> String {
    let percent = (received * 100).checked_div(total).unwrap_or(100);
    format!(
        "{filename}: {} of {} ({percent}%)",
        size_text(received),
        size_text(total)
    )
}

/// `rimix card get`: downloads one card file into `out`, resuming a partial
/// file an earlier try left there. Progress goes to stderr, a line updated
/// in place per chunk; the saved path to stdout.
pub async fn card_get(target: &Target, wanted: &str, out: &Path) -> Result<()> {
    std::fs::create_dir_all(out).map_err(|e| io_error("cannot create the output folder", e))?;
    let camera = open(target, None, Some(out), false, false).await?;
    let mut rx = camera.subscribe();
    let result = async {
        let card = camera.card_list().await?;
        let obj = resolve(&card, wanted)?;
        let download = camera.card_download(obj.handle);
        tokio::pin!(download);
        let show = |event: CameraEvent| {
            if let CameraEvent::CardDownloadProgress {
                filename,
                received,
                total,
                ..
            } = event
            {
                eprint!("\r{}", progress_text(&filename, received, total));
            }
        };
        let path = loop {
            tokio::select! {
                done = &mut download => break done,
                event = rx.recv() => if let Ok(event) = event { show(event) },
            }
        };
        // Progress sent just before the end is still queued.
        while let Ok(event) = rx.try_recv() {
            show(event);
        }
        eprintln!();
        let path = path?;
        println!("{}", path.display());
        Ok(())
    }
    .await;
    camera.disconnect().await;
    result
}

/// Whether a `Disconnected` reason means the camera refused this computer
/// for good (a refused SSH login or a changed fingerprint), so reconnecting
/// has stopped.
pub fn refusal(reason: &str) -> bool {
    reason.contains("refused the username or password") || reason.contains("fingerprint changed")
}

pub async fn watch(target: &Target, out: &Path, exit_after: Option<Duration>) -> Result<()> {
    let camera = open(target, None, Some(out), true, true).await?;
    let quit = async {
        match exit_after {
            Some(d) => tokio::time::sleep(d).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(quit);
    camera.set_destination(Destination {
        base: out.to_path_buf(),
        mode: DestinationMode::CustomFolder,
        recovery_dir: None,
    });
    let mut rx = camera.subscribe();
    println!("connected");
    let mut outcome = Ok(());
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = &mut quit => break,
            event = rx.recv() => match event {
                Ok(CameraEvent::Connected) => println!("connected"),
                Ok(CameraEvent::Disconnected { reason }) => {
                    println!("disconnected: {reason}");
                    if refusal(&reason) {
                        outcome = Err(anyhow::Error::new(CliError::Refused(reason)));
                        break;
                    }
                }
                Ok(CameraEvent::Reconnecting { attempt }) => println!("reconnecting #{attempt}"),
                Ok(CameraEvent::CaptureStarted { filename }) => println!("receiving {filename}"),
                Ok(CameraEvent::CaptureSaved(saved)) => println!("saved {}", saved.path.display()),
                Ok(CameraEvent::CaptureSkipped { filename }) => println!("skipped {filename}"),
                Ok(CameraEvent::CaptureFailed { filename, error }) => {
                    println!("failed {filename}: {error}")
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            },
        }
    }
    camera.disconnect().await;
    outcome
}

/// The Wi-Fi under `RIMIX_WIFI=fake` or the real one.
fn wifi() -> Wifi {
    Wifi::from_env(Arc::new(SystemRunner))
}

/// How a network name reads: the system (macOS especially) often hides it
/// even while joined.
fn network_text(name: Option<&str>, address: Option<&str>) -> String {
    match (name, address) {
        (Some(name), _) => name.to_string(),
        (None, Some(_)) => "hidden by the system".to_string(),
        (None, None) => "none".to_string(),
    }
}

/// `rimix wifi status`: the device, the network and the address.
pub async fn wifi_status() -> Result<()> {
    let wifi = wifi();
    let (device, network, address) = tokio::task::spawn_blocking(move || {
        Ok::<_, WifiError>((wifi.device()?, wifi.current_network()?, wifi.address()?))
    })
    .await??;
    println!("device: {device}");
    println!(
        "network: {}",
        network_text(network.as_deref(), address.as_deref())
    );
    println!("address: {}", address.as_deref().unwrap_or("none"));
    Ok(())
}

/// The first line of stdin without its line ending; `None` when empty.
pub fn read_password(input: impl std::io::BufRead) -> Result<Option<String>> {
    let mut line = String::new();
    let mut input = input;
    input
        .read_line(&mut line)
        .map_err(|e| CliError::Io(format!("Cannot read the password from stdin: {e}")))?;
    let password = line.trim_end_matches(['\r', '\n']).to_string();
    Ok((!password.is_empty()).then_some(password))
}

/// `rimix wifi join`: joins and prints what happened.
pub async fn wifi_join(ssid: String, password_stdin: bool) -> Result<()> {
    let password = if password_stdin {
        read_password(std::io::stdin().lock())?
    } else {
        None
    };
    let wifi = wifi();
    let report =
        tokio::task::spawn_blocking(move || wifi.join_report(&ssid, password.as_deref())).await??;
    println!("attempts: {}", report.attempts);
    println!(
        "network: {}",
        network_text(report.seen_ssid.as_deref(), report.address.as_deref())
    );
    println!("address: {}", report.address.as_deref().unwrap_or("none"));
    Ok(())
}

/// How long `ble handoff` and `ble shoot` listen for the camera.
const BLE_LOOKUP: Duration = Duration::from_secs(5);

/// The rows of `ble scan`, columns padded to line up.
pub fn ble_table(found: &[Found]) -> String {
    let mut rows = vec![[
        "name".to_string(),
        "model".to_string(),
        "pairing".to_string(),
        "address".to_string(),
    ]];
    for f in found {
        rows.push([
            f.advert.name.clone().unwrap_or_else(|| "-".to_string()),
            f.advert.model_code.clone(),
            if f.advert.pairing_open {
                "open"
            } else {
                "closed"
            }
            .to_string(),
            f.address.clone(),
        ]);
    }
    let width = |col: usize| {
        rows.iter()
            .map(|r| r[col].chars().count())
            .max()
            .unwrap_or(0)
    };
    let widths = [width(0), width(1), width(2)];
    rows.iter()
        .map(|r| {
            format!(
                "{:w0$}  {:w1$}  {:w2$}  {}\n",
                r[0],
                r[1],
                r[2],
                r[3],
                w0 = widths[0],
                w1 = widths[1],
                w2 = widths[2]
            )
        })
        .collect()
}

/// The one camera whose name or address is `wanted`, ignoring case.
pub fn pick_camera<'a>(found: &'a [Found], wanted: &str) -> Result<&'a Found> {
    let wanted_lower = wanted.to_lowercase();
    let matches: Vec<&Found> = found
        .iter()
        .filter(|f| {
            f.address.to_lowercase() == wanted_lower
                || f.advert.name.as_deref().map(str::to_lowercase) == Some(wanted_lower.clone())
        })
        .collect();
    match matches.as_slice() {
        [one] => Ok(one),
        [] => Err(anyhow::Error::new(BleError::NotFound)
            .context(format!("no Sony camera called {wanted:?} was heard"))),
        _ => Err(CliError::Usage(format!(
            "{} cameras are called {wanted:?}; give the address instead",
            matches.len()
        ))
        .into()),
    }
}

/// Listens and returns the camera called `wanted`.
async fn find_camera(adapter: &dyn BleAdapter, wanted: &str) -> Result<Found> {
    let found = adapter.scan(BLE_LOOKUP).await?;
    Ok(pick_camera(&found, wanted)?.clone())
}

fn camera_label(found: &Found) -> &str {
    found.advert.name.as_deref().unwrap_or(&found.address)
}

/// `rimix ble scan`: the Sony cameras heard.
pub async fn ble_scan(seconds: Duration) -> Result<()> {
    let adapter = rimix_ble::adapter_from_env().await?;
    let found = adapter.scan(seconds).await?;
    if found.is_empty() {
        println!("no Sony cameras heard");
    } else {
        print!("{}", ble_table(&found));
    }
    Ok(())
}

/// `rimix ble handoff`: the camera's Wi-Fi name; the password only as its
/// length.
pub async fn ble_handoff(wanted: &str) -> Result<()> {
    let adapter = rimix_ble::adapter_from_env().await?;
    let camera = find_camera(adapter.as_ref(), wanted).await?;
    let creds = rimix_ble::handoff(adapter.as_ref(), &camera.address).await?;
    println!("ssid: {}", creds.ssid);
    println!(
        "password: (hidden, {} chars)",
        creds.password.chars().count()
    );
    Ok(())
}

/// `rimix ble shoot`: one photo through the Bluetooth remote shutter.
pub async fn ble_shoot(wanted: &str) -> Result<()> {
    let adapter = rimix_ble::adapter_from_env().await?;
    let camera = find_camera(adapter.as_ref(), wanted).await?;
    rimix_ble::shoot(adapter.as_ref(), &camera.address).await?;
    println!("shot: {}", camera_label(&camera));
    Ok(())
}

/// How the camera should reach this computer: the Wi-Fi address, or a
/// placeholder when it is unknown.
fn lan_address(port: u16) -> String {
    match wifi().current_address() {
        Ok(Some(ip)) => format!("{ip}:{port}"),
        _ => format!("this computer's address:{port}"),
    }
}

/// `rimix sftp serve`: receives files until Ctrl-C, one line per file.
pub async fn sftp_serve(
    dir: &Path,
    port: u16,
    user: String,
    password: String,
    host_key: &Path,
) -> Result<()> {
    let host_key =
        rimix_sftp::load_or_create_host_key(host_key).map_err(|e| io_error("host key", e))?;
    let cfg = rimix_sftp::ReceiverConfig {
        bind: SocketAddr::from(([0, 0, 0, 0], port)),
        user: user.clone(),
        password,
        host_key,
        destination: dir.to_path_buf(),
    };
    let receiver = rimix_sftp::Receiver::start(cfg, |file| {
        println!(
            "received {} ({})",
            file.path.display(),
            size_text(file.bytes)
        );
    })
    .await
    .map_err(|e| {
        let text = match e {
            rimix_sftp::Error::Io(io) if io.kind() == std::io::ErrorKind::AddrInUse => {
                format!("Port {port} is in use")
            }
            e => format!("cannot start the SFTP server: {e}"),
        };
        anyhow::Error::new(CliError::Io(text))
    })?;
    let bound = receiver.local_addr().port();
    let address = tokio::task::spawn_blocking(move || lan_address(bound)).await?;
    println!("address: {address}");
    println!("user: {user}");
    println!("fingerprint: {}", receiver.fingerprint_sha256());
    let _ = tokio::signal::ctrl_c().await;
    receiver.stop().await;
    Ok(())
}

#[cfg(test)]
mod wifi_tests {
    use super::*;

    #[test]
    fn the_password_is_the_first_line_without_its_ending() {
        let read = |s: &str| read_password(std::io::Cursor::new(s.as_bytes())).unwrap();
        assert_eq!(read("12345678\r\nmore\n").as_deref(), Some("12345678"));
        assert_eq!(read("pass word\n").as_deref(), Some("pass word"));
        assert_eq!(read(""), None);
        assert_eq!(read("\n"), None);
    }

    #[test]
    fn a_refused_login_or_changed_fingerprint_ends_watch() {
        assert!(refusal("the camera refused the username or password"));
        assert!(refusal(
            "the camera's fingerprint changed (expected SHA256:a, got SHA256:b)"
        ));
        assert!(!refusal("connection reset by peer"));
        assert!(!refusal("Connection refused (os error 61)"));
    }

    #[test]
    fn a_hidden_name_with_an_address_says_so() {
        assert_eq!(network_text(Some("Home"), None), "Home");
        assert_eq!(
            network_text(None, Some("192.168.1.2")),
            "hidden by the system"
        );
        assert_eq!(network_text(None, None), "none");
    }
}

#[cfg(test)]
mod card_tests {
    use super::*;
    use sony_ptpip::CardKind;

    fn obj(handle: u32, filename: &str) -> CardObject {
        CardObject {
            handle,
            storage: 1,
            filename: filename.into(),
            bytes: 1,
            size_known: true,
            format: 0x3801,
            kind: CardKind::Still,
            captured: None,
        }
    }

    fn usage_text(r: Result<&CardObject>) -> String {
        let e = r.unwrap_err();
        assert!(matches!(
            e.downcast_ref::<CliError>(),
            Some(CliError::Usage(_))
        ));
        e.to_string()
    }

    #[test]
    fn a_file_is_found_by_handle_or_by_name_without_case() {
        let card = [obj(257, "DSC00010.JPG"), obj(258, "DSC00010.ARW")];
        assert_eq!(resolve(&card, "258").unwrap().handle, 258);
        assert_eq!(resolve(&card, "0x101").unwrap().handle, 257);
        assert_eq!(resolve(&card, "dsc00010.arw").unwrap().handle, 258);
        assert_eq!(
            usage_text(resolve(&card, "C0001.MP4")),
            "no file named C0001.MP4 on the card"
        );
    }

    #[test]
    fn two_files_with_one_name_need_the_handle() {
        let card = [obj(257, "DSC00010.JPG"), obj(0x0002_0001, "DSC00010.JPG")];
        assert!(usage_text(resolve(&card, "DSC00010.JPG")).contains("give the handle"));
    }

    #[test]
    fn progress_reads_received_of_total() {
        assert_eq!(
            progress_text("C0001.MP4", 1 << 20, 4 << 20),
            "C0001.MP4: 1.0 MB of 4.0 MB (25%)"
        );
        assert_eq!(progress_text("E", 0, 0), "E: 0 B of 0 B (100%)");
    }
}
