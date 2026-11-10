// User-facing copy that depends on what went wrong.

/** sony-ptpip's `Error::PairingTimeout` Display text. */
const PAIRING_TIMEOUT = 'did not confirm the pairing';

/** Signs that nothing answered at the address (macOS, Linux and Windows wording). */
const UNREACHABLE = [
  'refused',
  'timed out',
  'no route to host',
  'host is down',
  'unreachable',
  'disconnected',
];

/**
 * The connect screen's message for an error string from the `connect`
 * command (the Rust side's `Error` Display text, prefixed with
 * "Cannot connect to <addr>: "). Only the pairing wait asks for OK on the
 * camera; a connect timeout means the camera was not reached at all.
 */
export function connectErrorMessage(err: string, ip: string): string {
  const e = err.toLowerCase();
  if (e.includes(PAIRING_TIMEOUT)) {
    return 'The camera asked you to allow this computer. Press OK on the camera, then try again.';
  }
  if (UNREACHABLE.some((s) => e.includes(s))) {
    return `Could not reach the camera at ${ip}. Check that PC Remote is on and you are on the camera's Wi-Fi.`;
  }
  const detail = err.replace(/^Cannot connect to [^:]+(:\d+)?: /, '');
  return `Could not connect to the camera at ${ip}: ${detail}`;
}
