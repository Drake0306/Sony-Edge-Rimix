//! `rimix`: drive a Sony camera from the command line.

mod commands;
mod format;

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use commands::{CliError, Target};
use rimix_wifi::WifiError;
use sony_ptpip::error::Error;
use sony_ptpip::SshConfig;

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

/// Access Authentication, on every camera command.
#[derive(Args)]
struct SshArgs {
    /// Log in to the camera's SSH tunnel as this user (Access Authen. Info
    /// on the camera); the password is read from stdin.
    #[arg(long, value_name = "USER", requires = "ssh_password_stdin")]
    ssh_user: Option<String>,
    /// Read the SSH password from the first line of stdin.
    #[arg(long, requires = "ssh_user")]
    ssh_password_stdin: bool,
    /// The camera's SSH fingerprint, as shown on the camera
    /// (`SHA256:...` or MD5 `aa:bb:...`).
    #[arg(long, value_name = "FINGERPRINT", requires = "ssh_user")]
    fingerprint: Option<String>,
    /// The camera's SSH port.
    #[arg(long, default_value_t = 22, hide = true, value_name = "PORT")]
    ssh_port: u16,
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
        #[command(flatten)]
        ssh: SshArgs,
        /// Name shown on the camera's pairing prompt.
        #[arg(long)]
        name: Option<String>,
    },
    /// List the camera's properties.
    Props {
        #[arg(value_parser = parse_addr)]
        ip: SocketAddr,
        #[command(flatten)]
        ssh: SshArgs,
        /// Show every property, enabled or not.
        #[arg(long)]
        all: bool,
    },
    /// Set a property: `rimix set 192.168.1.5 D21E 1600`.
    Set {
        #[arg(value_parser = parse_addr)]
        ip: SocketAddr,
        #[command(flatten)]
        ssh: SshArgs,
        #[arg(value_parser = parse_code)]
        code: u16,
        value: String,
    },
    /// Move a property through its values: `rimix step 192.168.1.5 D21E +1`.
    Step {
        #[arg(value_parser = parse_addr)]
        ip: SocketAddr,
        #[command(flatten)]
        ssh: SshArgs,
        #[arg(value_parser = parse_code)]
        code: u16,
        #[arg(allow_hyphen_values = true)]
        delta: i32,
    },
    /// Take a picture and save it.
    Shoot {
        #[arg(value_parser = parse_addr)]
        ip: SocketAddr,
        #[command(flatten)]
        ssh: SshArgs,
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
    /// Start or stop movie recording.
    Record {
        #[arg(value_parser = parse_addr)]
        ip: SocketAddr,
        #[command(flatten)]
        ssh: SshArgs,
    },
    /// Save live view frames as JPEG files.
    Liveview {
        #[arg(value_parser = parse_addr)]
        ip: SocketAddr,
        #[command(flatten)]
        ssh: SshArgs,
        #[arg(long, default_value_t = 1)]
        frames: u32,
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
    /// Stay connected, save every new shot and print events until Ctrl-C.
    Watch {
        #[arg(value_parser = parse_addr)]
        ip: SocketAddr,
        #[command(flatten)]
        ssh: SshArgs,
        #[arg(long, default_value = ".")]
        out: PathBuf,
        /// Quit after this many seconds (for tests).
        #[arg(long, hide = true, value_name = "SECS")]
        exit_after: Option<u64>,
    },
    /// Show or join this computer's Wi-Fi network (add -v to see each command).
    Wifi {
        #[command(subcommand)]
        action: WifiCmd,
    },
}

#[derive(Subcommand)]
enum WifiCmd {
    /// Print the Wi-Fi device, the network it is on and its address.
    Status,
    /// Join a network and print how it went.
    Join {
        ssid: String,
        /// Read the password from the first line of stdin.
        #[arg(long)]
        password_stdin: bool,
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

/// 3 when the camera cannot be reached or its SSH fingerprint needs
/// confirming, 4 when it refuses (including its SSH login), 5 for local I/O
/// and capture failures, 2 for arguments clap cannot check, else 1. For
/// Wi-Fi: 3 when the network is not visible, 4 for any other failure.
fn exit_code(err: &anyhow::Error) -> i32 {
    if let Some(wifi) = err.downcast_ref::<WifiError>() {
        return match wifi {
            WifiError::NotVisible(_) => 3,
            WifiError::NoWifi | WifiError::Unsupported | WifiError::Failed(_) => 4,
        };
    }
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
            | Error::Protocol(_)
            | Error::HostKeyUnknown { .. },
        ) => 3,
        Some(
            Error::Response { .. }
            | Error::Unsupported(_)
            | Error::SshAuth
            | Error::HostKeyMismatch { .. },
        ) => 4,
        Some(Error::Recovered { .. }) => 5,
        None => 1,
    }
}

/// The tunnel settings from the SSH options, reading the password from
/// stdin; `None` without `--ssh-user`.
fn ssh_config(args: SshArgs) -> anyhow::Result<Option<SshConfig>> {
    let Some(user) = args.ssh_user else {
        return Ok(None);
    };
    let password = commands::read_password(std::io::stdin().lock())?
        .ok_or_else(|| CliError::Usage("--ssh-password-stdin found no password on stdin".into()))?;
    Ok(Some(SshConfig {
        user,
        password,
        expected_fingerprint: args.fingerprint,
        port: args.ssh_port,
    }))
}

/// What to print instead of the error, when the camera's SSH fingerprint
/// has not been confirmed yet.
fn fingerprint_prompt(err: &anyhow::Error) -> Option<String> {
    match err.downcast_ref::<Error>()? {
        Error::HostKeyUnknown { sha256, md5 } => Some(format!(
            "Fingerprint: {sha256} (md5 {md5})\nConfirm it on the camera and pass --fingerprint"
        )),
        _ => None,
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
    let target = |ip, ssh: SshArgs| -> anyhow::Result<Target> {
        Ok(Target {
            addr: ip,
            guid_file: guid_file.clone(),
            ssh: ssh_config(ssh)?,
        })
    };
    match cli.command {
        Cmd::Discover {
            timeout,
            ssdp_target,
        } => commands::discover(Duration::from_secs(timeout), ssdp_target).await,
        Cmd::Connect { ip, ssh, name } => commands::connect(&target(ip, ssh)?, name).await,
        Cmd::Props { ip, ssh, all } => commands::props(&target(ip, ssh)?, all).await,
        Cmd::Set {
            ip,
            ssh,
            code,
            value,
        } => commands::set(&target(ip, ssh)?, code, &value).await,
        Cmd::Step {
            ip,
            ssh,
            code,
            delta,
        } => commands::step(&target(ip, ssh)?, code, delta).await,
        Cmd::Shoot { ip, ssh, out } => commands::shoot(&target(ip, ssh)?, &out).await,
        Cmd::Record { ip, ssh } => commands::record(&target(ip, ssh)?).await,
        Cmd::Liveview {
            ip,
            ssh,
            frames,
            out,
        } => commands::liveview(&target(ip, ssh)?, frames, &out).await,
        Cmd::Watch {
            ip,
            ssh,
            out,
            exit_after,
        } => commands::watch(&target(ip, ssh)?, &out, exit_after.map(Duration::from_secs)).await,
        Cmd::Wifi { action } => match action {
            WifiCmd::Status => commands::wifi_status().await,
            WifiCmd::Join {
                ssid,
                password_stdin,
            } => commands::wifi_join(ssid, password_stdin).await,
        },
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
            match fingerprint_prompt(&err) {
                Some(prompt) => eprintln!("{prompt}"),
                None => eprintln!("error: {err:#}"),
            }
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

    #[test]
    fn wifi_not_visible_exits_3_and_other_failures_4() {
        let code = |e| exit_code(&anyhow::Error::new(e));
        assert_eq!(code(WifiError::NotVisible("X".into())), 3);
        assert_eq!(code(WifiError::Failed("X did not come up".into())), 4);
        assert_eq!(code(WifiError::NoWifi), 4);
        assert_eq!(code(WifiError::Unsupported), 4);
    }
}
