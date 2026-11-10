// User-facing copy that depends on what went wrong.

/**
 * The connect screen's message for an error string from the `connect`
 * command (the Rust side's `Error` Display text, prefixed with
 * "Cannot connect to <addr>: ").
 */
export function connectErrorMessage(err: string, ip: string): string {
  const e = err.toLowerCase();
  if (e.includes('refused') || e.includes('unreachable') || e.includes('disconnected')) {
    return `Could not reach the camera at ${ip}. Check that PC Remote is on and you are on the camera's Wi-Fi.`;
  }
  if (e.includes('timed out')) {
    return 'The camera asked you to allow this computer. Press OK on the camera, then try again.';
  }
  const detail = err.replace(/^Cannot connect to [^:]+(:\d+)?: /, '');
  return `Could not connect to the camera at ${ip}: ${detail}`;
}
