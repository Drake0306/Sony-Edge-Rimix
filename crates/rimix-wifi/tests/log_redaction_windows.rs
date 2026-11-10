//! A real Windows join never writes the password, or the profile XML that
//! holds it, to the log. This is the only test
//! in this binary on purpose: tracing caches whether a log line is wanted
//! per call site, and with a single scoped subscriber other test threads
//! (which have none) would cache "off" for the lines checked here.

#![cfg(target_os = "windows")]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rimix_wifi::{Output, Runner, Wifi};

const SSID: &str = "DIRECT-abc1:ZV-E10";

struct Scripted(Mutex<VecDeque<&'static str>>);

impl Runner for Scripted {
    fn run(&self, _: &str, _: &[&str]) -> std::io::Result<Output> {
        let stdout = self.0.lock().unwrap().pop_front().expect("unscripted call");
        Ok(Output {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }
}

/// Collects what the subscriber writes.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_join_never_logs_the_password() {
    let runner = Arc::new(Scripted(Mutex::new(VecDeque::from([
        "    Name                   : Wi-Fi\r\n    State                  : disconnected\r\n",
        "Profile DIRECT-abc1:ZV-E10 is added on interface Wi-Fi.\r\n",
        "The network specified by profile \"DIRECT-abc1:ZV-E10\" is not available to connect.\r\n",
        "Connection request was completed successfully.\r\n",
        "    Name                   : Wi-Fi\r\n    State                  : connected\r\n    SSID                   : DIRECT-abc1:ZV-E10\r\n",
    ]))));
    let wifi = Wifi::new(runner).with_clock(Instant::now, |_| {});
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let report = tracing::subscriber::with_default(subscriber, || {
        wifi.join_report(SSID, Some("s3cretpw")).unwrap()
    });
    assert_eq!(report.attempts, 2);
    let text = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(text.contains("netsh wlan add profile filename="), "{text}");
    assert!(
        text.contains("netsh wlan connect name=DIRECT-abc1:ZV-E10"),
        "{text}"
    );
    assert!(!text.contains("s3cretpw"), "{text}");
    assert!(!text.contains("keyMaterial"), "{text}");
}
