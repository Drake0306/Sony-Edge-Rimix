//! Running a command with its command line and output in the log. Secret
//! argument positions are replaced before anything is logged.

use crate::{redact, Output, Runner, WifiError};

/// Runs `args[0]` with the rest, logging the command line (secrets replaced)
/// and its trimmed output at `info`.
pub(crate) fn run_logged(
    runner: &dyn Runner,
    args: &[&str],
    secret: &[usize],
) -> Result<Output, WifiError> {
    run_with_log(runner, args, secret, false)
}

/// Runs a command and logs it; a `long_output` is logged at `debug` only.
pub(crate) fn run_with_log(
    runner: &dyn Runner,
    args: &[&str],
    secret: &[usize],
    long_output: bool,
) -> Result<Output, WifiError> {
    let cmd = redact(args, secret);
    match runner.run(args[0], &args[1..]) {
        Ok(out) => {
            let text = format!("{}{}", out.stdout, out.stderr);
            if long_output {
                tracing::debug!(cmd = %cmd, output = %text.trim(), "{}", args[0]);
            } else {
                tracing::info!(cmd = %cmd, output = %text.trim(), "{}", args[0]);
            }
            Ok(out)
        }
        Err(e) => {
            tracing::warn!(cmd = %cmd, error = %e, "{}", args[0]);
            Err(WifiError::Failed(format!("Could not run {}: {e}", args[0])))
        }
    }
}
