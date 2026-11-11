// User-facing copy that depends on what went wrong.

import type { Found } from './ipc';

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
  // Before the unreachable check: a refused login says "refused" too.
  const ssh = sshErrorMessage(err);
  if (ssh) return ssh;
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

/** sony-ptpip's `Error::SshAuth` Display text. */
const SSH_AUTH = 'the camera refused the username or password';
/** How sony-ptpip's `Error::HostKeyMismatch` Display text starts. */
const HOST_KEY_CHANGED = "the camera's fingerprint changed";

/**
 * The message for an Access Authentication failure (a refused login or a
 * changed fingerprint) in a connect error or a `Disconnected` reason, or
 * null for any other error.
 */
export function sshErrorMessage(err: string): string | null {
  if (err.includes(SSH_AUTH)) {
    return 'The camera refused the username or password. Check Access Authen. Info on the camera and try again.';
  }
  if (err.includes(HOST_KEY_CHANGED)) {
    return "The camera's fingerprint changed. If you reset the camera, forget it in App settings and connect again; otherwise do not connect.";
  }
  return null;
}

/** The connect screen's confirmation line for a camera's SSH fingerprint. */
export const fingerprintText = (sha256: string, md5: string) =>
  `This camera's fingerprint is ${sha256}${md5 ? ` (MD5 ${md5})` : ''}. Check it against Access Authen. Info on the camera; it may show either form.`;

/** The connect screen's warning when a camera's SSH fingerprint differs from the confirmed one. */
export const fingerprintChangedText = (expected: string, actual: string) =>
  `This camera's fingerprint changed. Stored: ${expected}. Camera now: ${actual}. If you reset the camera, forget it in App settings and connect again; otherwise do not connect.`;

/** The scan card's line when `RIMIX_WEBCAM=off` keeps the webcam off. */
export const WEBCAM_OFF = 'Webcam off for this run';

/** The connect screen's message for a webcam error (a DOMException name from getUserMedia). */
export function webcamErrorMessage(e: unknown): string {
  const name = e instanceof Error ? e.name : '';
  if (name === 'NotAllowedError') {
    return 'Rimix needs permission to use the webcam. Allow it in System Settings, Privacy & Security, Camera, then try again.';
  }
  if (name === 'NotFoundError' || name === 'OverconstrainedError') {
    return "No webcam found. Paste the code's text or type the name and password instead.";
  }
  return "The webcam isn't available here. Paste the code's text or type the name and password instead.";
}

/** The connect screen's progress lines while connecting over Wi-Fi. */
export const joiningText = (ssid: string) => `Joining ${ssid}`;
export const findingText = (ssid: string) => `Looking for the camera on ${ssid}`;
export const askToJoinText = (ssid: string) =>
  `Join ${ssid} from the Wi-Fi menu. Rimix connects as soon as the camera appears.`;
export const connectingText = (name: string) => `Connecting to ${name}. If the camera asks, press OK on it.`;

/** The Rust side's `WifiError::NoWifi` Display text. */
const NO_WIFI = 'This computer has no Wi-Fi';
/** How the Rust side's `WifiError::NotVisible` starts. */
const CANT_SEE = "This computer can't see the network";

/** The Rust side's `LOCAL_NETWORK_BLOCKED`: the error when the system keeps Rimix off the local network. */
const LOCAL_NETWORK_BLOCKED = 'This computer is blocking Rimix from the local network';
export const isLocalNetworkBlocked = (err: string) => err === LOCAL_NETWORK_BLOCKED;
/** What to do about it; also the Nearby card's line while it lasts. */
export const ALLOW_LOCAL_NETWORK =
  'macOS is keeping Rimix off the local network, so it cannot reach the camera. Open System Settings, Privacy & Security, Local Network, turn Rimix on (if it is on, turn it off and on again), then quit and reopen Rimix.';

/**
 * The connect screen's message when joining the camera's network or finding
 * the camera on it fails. `wait` is finding it after the user was asked to
 * join the network themselves.
 */
export function wifiErrorMessage(step: 'join' | 'find' | 'wait', ssid: string, detail: string): string {
  if (step !== 'join' && detail === LOCAL_NETWORK_BLOCKED) {
    const lead = ALLOW_LOCAL_NETWORK[0].toLowerCase() + ALLOW_LOCAL_NETWORK.slice(1);
    return step === 'find' ? `Joined ${ssid}, but ${lead}` : ALLOW_LOCAL_NETWORK;
  }
  if (step === 'wait') {
    return `No camera answered on ${ssid}. Check this computer is on that network, then on the camera open Menu, Network, PC Remote Function, and stay on that screen.`;
  }
  if (step === 'find') {
    return `Joined ${ssid} but no camera answered. On the camera, open Menu, Network, PC Remote Function, and stay on that screen.`;
  }
  if (detail.startsWith(CANT_SEE)) {
    return `This computer can't see ${ssid} yet. On the camera, open Menu, Network, PC Remote Function, Wi-Fi Direct Info and stay on that screen, then try again.`;
  }
  if (detail === NO_WIFI) {
    return 'This computer has no Wi-Fi. Put the camera and this computer on the same router and use Nearby.';
  }
  return `Could not join ${ssid}: ${detail.replace(/\.+$/, '')}. Join it from the Wi-Fi menu, then try again.`;
}

export const NOT_A_WIFI_CODE = "This code isn't a camera Wi-Fi code. Check the text below, or type the name and password.";

/** Top bar text while reconnecting; from attempt 5 it says what to do by hand. */
export function reconnectingText(attempt: number): string {
  return attempt >= 5
    ? "Reconnecting… if the camera's Wi-Fi dropped, rejoin it from the Wi-Fi menu"
    : 'Reconnecting';
}

/** What the Nearby card calls a camera heard over Bluetooth. */
export const bleCameraName = (f: Found) => f.advert.name || `Sony camera ${f.advert.model_code}`;
export const bleNearbyText = (f: Found) => `${bleCameraName(f)} nearby`;
export const PAIRING_OPEN = 'pairing open';
export const PAIRING_OFF = 'pairing off';
export const JOIN_VIA_BLUETOOTH = 'Join via Bluetooth';
/** The connect screen's progress line while the Wi-Fi details are read. */
export const bleReadingText = (name: string) => `Reading Wi-Fi details from ${name} over Bluetooth`;

/** How the Rust side's `BleError::Unavailable` Display text starts. */
const BLE_UNAVAILABLE_PREFIX = 'Bluetooth unavailable:';
/** How the Rust side's `BleError::Io` Display text starts. */
const BLE_FAILED_PREFIX = 'Bluetooth failed: ';

/** Whether a Bluetooth command failed because Bluetooth is off, missing or not allowed. */
export const isBleUnavailable = (err: string) => err.startsWith(BLE_UNAVAILABLE_PREFIX);

/** The Nearby card's quiet line when Bluetooth cannot be used. */
export const BLE_UNAVAILABLE = 'Bluetooth is off or not allowed; nearby cameras come from Wi-Fi only.';

/** The Nearby card's line under a camera whose Wi-Fi details could not be read. */
export function bleFailedText(err: string): string {
  const bare = err.startsWith(BLE_FAILED_PREFIX) ? err.slice(BLE_FAILED_PREFIX.length) : err;
  const detail = bare.replace(/\.+$/, '');
  return `Rimix could not read the Wi-Fi details over Bluetooth (${detail}). Scan the QR code or type the name and password instead.`;
}

/** The toast when a movie cannot be copied because the camera keeps its card to itself. */
export const MOVIES_STAY_ON_CARD =
  'Videos stay on the card: this camera does not let its card be read while Rimix controls it.';
/** The Save to panel's line while the camera keeps new pictures on its card only. */
export const SHOTS_STAY_ON_CARD =
  'The camera is keeping pictures on its card only. On the camera, set Still Img. Save Dest. to PC+Camera.';
