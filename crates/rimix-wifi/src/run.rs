//! Running a command with its command line and output in the log. Secret
//! argument positions are replaced before anything is logged.

use crate::{redact, Output, Runner, WifiError};

/// Runs `args[0]` with the rest, logging the command line (secrets replaced)
/// and its trimmed output at `info`.
#[cfg(target_os = "macos")]
pub(crate) fn run_logged(
    runner: &dyn Runner,
    args: &[&str],
    secret: &[usize],
) -> Result<Output, WifiError> {
    run_with_log(runner, args, secret, false)
}

/// Runs `args[0]` with the rest and one `key=value` argument per pair (see
/// [`Runner::run_kv`]), logging the command line at `info`. No secrets may go
/// in either.
#[cfg(target_os = "windows")]
pub(crate) fn run_kv_logged(
    runner: &dyn Runner,
    args: &[&str],
    pairs: &[(&str, &str)],
) -> Result<Output, WifiError> {
    let shown: Vec<String> = args
        .iter()
        .map(|a| a.to_string())
        .chain(pairs.iter().map(|(k, v)| format!("{k}={v}")))
        .collect();
    let cmd = shown.join(" ");
    log_result(
        &cmd,
        args[0],
        false,
        runner.run_kv(args[0], &args[1..], pairs),
    )
}

/// Runs a command and logs it; a `long_output` is logged at `debug` only.
pub(crate) fn run_with_log(
    runner: &dyn Runner,
    args: &[&str],
    secret: &[usize],
    long_output: bool,
) -> Result<Output, WifiError> {
    let cmd = redact(args, secret);
    log_result(&cmd, args[0], long_output, runner.run(args[0], &args[1..]))
}

fn log_result(
    cmd: &str,
    program: &str,
    long_output: bool,
    result: std::io::Result<Output>,
) -> Result<Output, WifiError> {
    match result {
        Ok(out) => {
            let text = format!("{}{}", out.stdout, out.stderr);
            if long_output {
                tracing::debug!(cmd = %cmd, output = %text.trim(), "{program}");
            } else {
                tracing::info!(cmd = %cmd, output = %text.trim(), "{program}");
            }
            Ok(out)
        }
        Err(e) => {
            tracing::warn!(cmd = %cmd, error = %e, "{program}");
            Err(WifiError::Failed(format!("Could not run {program}: {e}")))
        }
    }
}
