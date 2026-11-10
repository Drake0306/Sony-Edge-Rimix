//! Subcommand bodies. Each connects, does its work and disconnects.

use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use rimix_wifi::{SystemRunner, Wifi, WifiError};
use sony_ptpip::capture::{Destination, DestinationMode};
use sony_ptpip::ptp::PtpValue;
use sony_ptpip::{
    default_host_name, load_or_create_guid, Camera, CameraConfig, CameraEvent, PropertySummary,
};
use tokio::sync::broadcast::{error::RecvError, Receiver};
use tokio::time::{timeout_at, Instant};

use crate::format::{hex_code, props_table, DOCK_PROPS};

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
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Usage(m) | CliError::Io(m) => f.write_str(m),
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
        ssh: None,
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
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = &mut quit => break,
            event = rx.recv() => match event {
                Ok(CameraEvent::Connected) => println!("connected"),
                Ok(CameraEvent::Disconnected { reason }) => println!("disconnected: {reason}"),
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
    Ok(())
}

/// The Wi-Fi under `RIMIX_WIFI=fake` or the real one.
fn wifi() -> Wifi {
    Wifi::from_env(Arc::new(SystemRunner))
}

/// How a network name reads: macOS often hides it even while joined.
fn network_text(name: Option<&str>, address: Option<&str>) -> String {
    match (name, address) {
        (Some(name), _) => name.to_string(),
        (None, Some(_)) => "hidden by macOS".to_string(),
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
fn read_password(input: impl std::io::BufRead) -> Result<Option<String>> {
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
    fn a_hidden_name_with_an_address_says_so() {
        assert_eq!(network_text(Some("Home"), None), "Home");
        assert_eq!(network_text(None, Some("192.168.1.2")), "hidden by macOS");
        assert_eq!(network_text(None, None), "none");
    }
}
