//! `rimix`: drive a Sony camera from the command line.

mod commands;
mod format;

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use commands::{CliError, Target};
use sony_ptpip::error::Error;

#[derive(Parser)]
#[command(
    name = "rimix",
    version,
    about = "Control a Sony Alpha camera over PTP/IP"
)]
struct Cli {
    /// Where the host identity is kept (default: the Rimix config folder).
    #[arg(long, global = true, value_name = "PATH")]
    guid_file: Option<PathBuf>,
    /// Log what happens to stderr.
    #[arg(short, long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Find cameras on the network.
    Discover {
        /// Seconds to wait for answers.
        #[arg(long, default_value_t = 3)]
        timeout: u64,
        /// Send the SSDP search to this address instead of the multicast group.
        #[arg(long, hide = true, value_name = "ADDR")]
        ssdp_target: Option<SocketAddr>,
    },
    /// Connect, print model, firmware and battery, and exit.
    Connect {
        #[arg(value_parser = parse_addr)]
        ip: SocketAddr,
        /// Name shown on the camera's pairing prompt.
        #[arg(long)]
        name: Option<String>,
    },
    /// List the camera's properties.
    Props {
        #[arg(value_parser = parse_addr)]
        ip: SocketAddr,
        /// Show every property, enabled or not.
        #[arg(long)]
        all: bool,
    },
    /// Set a property: `rimix set 192.168.1.5 D21E 1600`.
    Set {
        #[arg(value_parser = parse_addr)]
        ip: SocketAddr,
        #[arg(value_parser = parse_code)]
        code: u16,
        value: String,
    },
    /// Move a property through its values: `rimix step 192.168.1.5 D21E +1`.
    Step {
        #[arg(value_parser = parse_addr)]
        ip: SocketAddr,
        #[arg(value_parser = parse_code)]
        code: u16,
        #[arg(allow_hyphen_values = true)]
        delta: i32,
    },
    /// Take a picture and save it.
    Shoot {
        #[arg(value_parser = parse_addr)]
        ip: SocketAddr,
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
    /// Start or stop movie recording.
    Record {
        #[arg(value_parser = parse_addr)]
        ip: SocketAddr,
    },
    /// Save live view frames as JPEG files.
    Liveview {
        #[arg(value_parser = parse_addr)]
        ip: SocketAddr,
        #[arg(long, default_value_t = 1)]
        frames: u32,
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
    /// Stay connected, save every new shot and print events until Ctrl-C.
    Watch {
        #[arg(value_parser = parse_addr)]
        ip: SocketAddr,
        #[arg(long, default_value = ".")]
        out: PathBuf,
        /// Quit after this many seconds (for tests).
        #[arg(long, hide = true, value_name = "SECS")]
        exit_after: Option<u64>,
    },
}

/// `ip` or `ip:port`; the port defaults to 15740.
fn parse_addr(text: &str) -> Result<SocketAddr, String> {
    if let Ok(addr) = text.parse::<SocketAddr>() {
        return Ok(addr);
    }
    text.parse::<IpAddr>()
        .map(|ip| SocketAddr::new(ip, sony_ptpip::discovery::PTP_PORT))
        .map_err(|_| format!("{text:?} is not an IP address like 192.168.1.5 or 192.168.1.5:15740"))
}

/// A property code in hex, with or without `0x`.
fn parse_code(text: &str) -> Result<u16, String> {
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    u16::from_str_radix(digits, 16).map_err(|_| format!("{text:?} is not a hex property code"))
}

/// 3 when the camera cannot be reached, 4 when it refuses, 5 for local I/O
/// and capture failures, 2 for arguments clap cannot check, else 1.
fn exit_code(err: &anyhow::Error) -> i32 {
    if let Some(cli) = err.downcast_ref::<CliError>() {
        return match cli {
            CliError::Usage(_) => 2,
            CliError::Io(_) => 5,
        };
    }
    match err.downcast_ref::<Error>() {
        Some(
            Error::Io(_)
            | Error::Timeout
            | Error::PairingTimeout
            | Error::Disconnected
            | Error::Protocol(_),
        ) => 3,
        Some(Error::Response { .. } | Error::Unsupported(_)) => 4,
        None => 1,
    }
}

fn guid_path(arg: Option<PathBuf>) -> PathBuf {
    arg.unwrap_or_else(|| {
        directories::ProjectDirs::from("", "", "Rimix")
            .map(|d| d.config_dir().join("guid"))
            .unwrap_or_else(|| PathBuf::from("rimix-guid"))
    })
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    let guid_file = guid_path(cli.guid_file);
    let target = |ip| Target {
        addr: ip,
        guid_file: guid_file.clone(),
    };
    match cli.command {
        Cmd::Discover {
            timeout,
            ssdp_target,
        } => commands::discover(Duration::from_secs(timeout), ssdp_target).await,
        Cmd::Connect { ip, name } => commands::connect(&target(ip), name).await,
        Cmd::Props { ip, all } => commands::props(&target(ip), all).await,
        Cmd::Set { ip, code, value } => commands::set(&target(ip), code, &value).await,
        Cmd::Step { ip, code, delta } => commands::step(&target(ip), code, delta).await,
        Cmd::Shoot { ip, out } => commands::shoot(&target(ip), &out).await,
        Cmd::Record { ip } => commands::record(&target(ip)).await,
        Cmd::Liveview { ip, frames, out } => commands::liveview(&target(ip), frames, &out).await,
        Cmd::Watch {
            ip,
            out,
            exit_after,
        } => commands::watch(&target(ip), &out, exit_after.map(Duration::from_secs)).await,
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let default = if cli.verbose { "info" } else { "off" };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(exit_code(&err) as u8)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_link_errors_exit_3() {
        for e in [Error::Timeout, Error::PairingTimeout, Error::Disconnected] {
            assert_eq!(exit_code(&anyhow::Error::new(e)), 3);
        }
    }
}
