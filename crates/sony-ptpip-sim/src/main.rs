//! Runs the simulated camera until Ctrl-C: `sony-ptpip-sim [--port 15740]
//! [--ssdp] [--fixture PATH] [--ssh USER:PASSWORD]`.

use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::process::ExitCode;

use sony_ptpip_sim::{spawn, SimCommand, SimConfig, SshSimConfig};

const SSDP_PORT: u16 = 1900;

fn usage() -> ExitCode {
    eprintln!(
        "usage: sony-ptpip-sim [--port 15740] [--ssdp] [--fixture PATH] [--ssh USER:PASSWORD]"
    );
    ExitCode::from(2)
}

#[tokio::main]
async fn main() -> ExitCode {
    let mut port: u16 = 15740;
    let mut ssdp = false;
    let mut config = SimConfig::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => match args.next().and_then(|v| v.parse().ok()) {
                Some(p) => port = p,
                None => return usage(),
            },
            "--ssdp" => ssdp = true,
            "--fixture" => match args.next() {
                Some(path) => config.fixture = PathBuf::from(path),
                None => return usage(),
            },
            "--ssh" => match args.next().as_deref().and_then(|v| v.split_once(':')) {
                Some((user, password)) if !user.is_empty() => {
                    config.ssh = Some(SshSimConfig {
                        user: user.into(),
                        password: password.into(),
                    });
                }
                _ => return usage(),
            },
            _ => return usage(),
        }
    }
    config.bind_addr = Some(SocketAddr::from(([0, 0, 0, 0], port)));
    if ssdp {
        // Check the port first: starting the responder on a taken or
        // privileged port would abort the whole simulator.
        match UdpSocket::bind(("127.0.0.1", SSDP_PORT)) {
            Ok(probe) => {
                drop(probe);
                config.ssdp_port = Some(SSDP_PORT);
            }
            Err(e) => eprintln!("warning: SSDP disabled, cannot bind UDP {SSDP_PORT}: {e}"),
        }
    }
    let sim = spawn(config).await;
    println!("Simulator listening on {}", sim.addr);
    if let Some(addr) = sim.ssh_addr {
        println!("SSH front listening on {addr}");
        if let Some(fp) = &sim.fingerprint_sha256 {
            println!("SSH host key {fp}");
        }
        if let Some(fp) = &sim.fingerprint_md5 {
            println!("SSH host key MD5:{fp}");
        }
    }
    let _ = tokio::signal::ctrl_c().await;
    let _ = sim.events.send(SimCommand::Shutdown).await;
    ExitCode::SUCCESS
}
