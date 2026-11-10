// Typed wrappers for the Rust commands in src-tauri/src/commands.rs.
//
// Shapes follow serde's defaults for the sony-ptpip types, which have no
// renames: fields are snake_case and enums are externally tagged with the
// variant name as written in Rust. So a `CameraEvent` arrives as
//
//   "Connected" | "Frame"                         (unit variants: bare strings)
//   {"Disconnected": {"reason": "..."}}           (struct variants: one key)
//   {"StateChanged": {"connected": true, ...}}    (newtype variants: one key)
//
// and a `PtpValue` as {"U16": 5}. Each event reaches the webview tagged with
// the camera it came from: {"camera_id": "...", "event": <CameraEvent>}.
// The app's own types (AppSettings and
// RecentCamera) use camelCase fields. `src/fixtures/state-changed.json` and
// `src/fixtures/capture-saved.json` are pinned by the Rust tests
// `state_changed_event_json_is_stable` and `capture_saved_event_json_is_stable`.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type PtpValue =
  | { I8: number }
  | { U8: number }
  | { I16: number }
  | { U16: number }
  | { I32: number }
  | { U32: number }
  | { I64: number }
  | { U64: number }
  | { Str: string }
  | { Array: PtpValue[] };

export type Enabled = 'Off' | 'On' | 'DisplayOnly';

export interface Choice {
  raw: PtpValue;
  label: string;
}

export interface PropertySummary {
  code: number;
  label: string;
  short: string;
  raw: PtpValue;
  writable: boolean;
  enabled: Enabled;
  choices: Choice[];
}

export interface CameraState {
  connected: boolean;
  /** True while the session runs through the camera's SSH tunnel. */
  secured: boolean;
  model: string;
  firmware: string;
  /** "3" or "4". */
  generation: string;
  battery: number | null;
  recording: boolean;
  focus_found: boolean;
  properties: PropertySummary[];
  live_view_fps: number;
  /** Milliseconds since the Unix epoch. */
  last_shot_at: number | null;
}

export interface Received {
  path: string;
  bytes: number;
  /** The saved file's name, including any `-N` suffix. */
  filename: string;
  /** The camera's name for the file; progress events use this name. */
  camera_filename: string;
}

export type CameraEvent =
  | 'Connected'
  | 'Frame'
  | { Disconnected: { reason: string } }
  | { Reconnecting: { attempt: number } }
  | { StateChanged: CameraState }
  | { CaptureStarted: { filename: string } }
  | { CaptureProgress: { filename: string; received: number; total: number } }
  | { CaptureSaved: Received }
  | { CaptureSkipped: { filename: string } }
  | { CaptureFailed: { filename: string; error: string } };

type Tagged<K extends string> = Extract<CameraEvent, Record<K, unknown>>;

function hasTag<K extends string>(e: CameraEvent, tag: K): e is Tagged<K> {
  return typeof e === 'object' && e !== null && tag in e;
}

export const isConnected = (e: CameraEvent): e is 'Connected' => e === 'Connected';
export const isDisconnected = (e: CameraEvent): e is Tagged<'Disconnected'> =>
  hasTag(e, 'Disconnected');
export const isReconnecting = (e: CameraEvent): e is Tagged<'Reconnecting'> =>
  hasTag(e, 'Reconnecting');
export const isStateChanged = (e: CameraEvent): e is Tagged<'StateChanged'> =>
  hasTag(e, 'StateChanged');
export const isCaptureStarted = (e: CameraEvent): e is Tagged<'CaptureStarted'> =>
  hasTag(e, 'CaptureStarted');
export const isCaptureProgress = (e: CameraEvent): e is Tagged<'CaptureProgress'> =>
  hasTag(e, 'CaptureProgress');
export const isCaptureSaved = (e: CameraEvent): e is Tagged<'CaptureSaved'> =>
  hasTag(e, 'CaptureSaved');
export const isCaptureSkipped = (e: CameraEvent): e is Tagged<'CaptureSkipped'> =>
  hasTag(e, 'CaptureSkipped');
export const isCaptureFailed = (e: CameraEvent): e is Tagged<'CaptureFailed'> =>
  hasTag(e, 'CaptureFailed');

/** A camera's id: its Wi-Fi name when connected through it, else `ip:port`. */
export type CameraId = string;

/** A camera event as the `camera-event` Tauri event carries it. */
export interface TaggedEvent {
  camera_id: CameraId;
  event: CameraEvent;
}

/** What `connect` resolves: the new camera's id and state. */
export interface ConnectResult {
  id: CameraId;
  state: CameraState;
}

/** One connected camera, as `list_cameras` lists it. */
export interface CameraSummary {
  id: CameraId;
  label: string;
  connected: boolean;
  secured: boolean;
  active: boolean;
}

export type DiscoverySource = 'Ssdp' | 'Gateway' | 'Manual';

export interface DiscoveredCamera {
  model: string;
  friendly_name: string;
  ip: string;
  ptp_port: number;
  ptp_version: string | null;
  pairing_required: boolean | null;
  ssh_supported: boolean | null;
  source: DiscoverySource;
}

export interface FocusFrame {
  kind: number;
  state: number;
  cx: number;
  cy: number;
  w: number;
  h: number;
}

export interface LiveViewMeta {
  focus_frames: FocusFrame[];
  level_deg: number | null;
  fps: number;
}

export type Appearance = 'Dark' | 'Light' | 'System';
export type Layout = 'Dock' | 'Ring';
export type DestinationMode = 'CustomFolder' | 'LightroomWatched' | 'FolderPerDate';

export interface RecentCamera {
  ip: string;
  model: string;
  /** Milliseconds since the Unix epoch. */
  lastUsed: number;
  /** The camera's Wi-Fi network name, when known. Never its password. */
  ssid: string | null;
  /** The Access Authentication username, for a camera reached through SSH. Never the password. */
  sshUser?: string | null;
  /** The SSH host key fingerprint the user confirmed for this camera. */
  sshFingerprint?: string | null;
}

/**
 * The login for a camera with Access Authentication. The password travels
 * only in the `connect` call; it is never stored. `fingerprint` is the one
 * the user confirmed, or null on a first connect.
 */
export interface SshLogin {
  user: string;
  password: string;
  fingerprint: string | null;
}

export type WifiJoin = 'Automatic' | 'Ask';

export interface PresetValue {
  code: number;
  raw: PtpValue;
}

/** A saved set of camera properties for one model; `slot` is 1 to 4. */
export interface Preset {
  id: string;
  name: string;
  model: string;
  slot: number;
  values: PresetValue[];
}

/** How one value of a preset went: `error` is null when the camera took it. */
export interface ApplyOutcome {
  code: number;
  error: string | null;
}

export interface AppSettings {
  appearance: Appearance;
  layout: Layout;
  destinationMode: DestinationMode;
  destinationBase: string;
  recentCameras: RecentCamera[];
  lastIp: string | null;
  wifiJoin: WifiJoin;
  webcamId: string | null;
  presets: Preset[];
}

export const discover = (timeoutMs: number) =>
  invoke<DiscoveredCamera[]>('discover', { timeoutMs });
/** Joins a Wi-Fi network and waits until it is up. The password is never stored. */
export const joinNetwork = (ssid: string, password: string | null) =>
  invoke<void>('join_network', { ssid, password });
/** The Wi-Fi network this computer is on, or null. */
export const currentNetwork = () => invoke<string | null>('current_network');
/** Opens the folder that holds `rimix.log`. */
export const openLogs = () => invoke<void>('open_logs');
/** Waits up to `timeoutMs` for a camera to answer on the network. */
export const waitForCamera = (timeoutMs: number) =>
  invoke<DiscoveredCamera>('wait_for_camera', { timeoutMs });
/** Adds `cameraId` to a command's arguments when one is named; none means the active camera. */
const forCamera = <T extends object>(args: T, cameraId: CameraId | null) =>
  cameraId === null ? args : { ...args, cameraId };
/** Calls a command that takes only a camera; with none named it gets no arguments at all. */
const onCamera = <T>(cmd: string, cameraId: CameraId | null) =>
  cameraId === null ? invoke<T>(cmd) : invoke<T>(cmd, { cameraId });

/**
 * `ip` is an IP address (port 15740) or `ip:port`. `ssid` is the camera's
 * Wi-Fi name, remembered with the camera when known. With `ssh` the session
 * runs through the camera's Access Authentication tunnel; when the
 * fingerprint needs confirming it rejects with `HOSTKEY <sha256> <md5>`.
 * Other cameras stay connected; the new one becomes the active camera.
 */
export const connect = (ip: string, ssid: string | null = null, ssh: SshLogin | null = null) =>
  invoke<ConnectResult>('connect', { ip, ssid, ssh });
/** Disconnects one camera, the active one when none is named. */
export const disconnect = (cameraId: CameraId | null = null) => onCamera<void>('disconnect', cameraId);
export const disconnectAll = () => invoke<void>('disconnect_all');
export const listCameras = () => invoke<CameraSummary[]>('list_cameras');
/** Makes `id` the camera that commands naming none act on. */
export const setActiveCamera = (id: CameraId) => invoke<void>('set_active_camera', { id });
/** Shoots every connected camera at once: `[id, error or null]` for each. */
export const shootAll = () => invoke<[CameraId, string | null][]>('shoot_all');
export const getState = (cameraId: CameraId | null = null) => onCamera<CameraState | null>('get_state', cameraId);
export const setProperty = (code: number, raw: PtpValue, cameraId: CameraId | null = null) =>
  invoke<void>('set_property', forCamera({ code, raw }, cameraId));
export const stepProperty = (code: number, delta: number, cameraId: CameraId | null = null) =>
  invoke<void>('step_property', forCamera({ code, delta }, cameraId));
/** Writes each value in order, carrying on past refusals; one outcome per value. */
export const applyValues = (values: PresetValue[], cameraId: CameraId | null = null) =>
  invoke<ApplyOutcome[]>('apply_values', forCamera({ values }, cameraId));
export const shoot = (cameraId: CameraId | null = null) => onCamera<void>('shoot', cameraId);
export const focus = (cameraId: CameraId | null = null) => onCamera<void>('focus', cameraId);
export const toggleRecord = (cameraId: CameraId | null = null) => onCamera<void>('toggle_record', cameraId);
/** The active camera's latest live view JPEG; empty when there is no frame yet. */
export const liveViewFrame = (cameraId: CameraId | null = null) => onCamera<ArrayBuffer>('live_view_frame', cameraId);
export const liveViewMeta = (cameraId: CameraId | null = null) => onCamera<LiveViewMeta>('live_view_meta', cameraId);
export const getSettings = () => invoke<AppSettings>('get_settings');
export const saveSettings = (settings: AppSettings) =>
  invoke<void>('save_settings', { settings });
/** A saved JPEG's bytes for the filmstrip; rejects for RAW files and files over 25 MB. */
export const readThumbnail = (path: string) => invoke<ArrayBuffer>('read_thumbnail', { path });
/** `null` when the dialog is cancelled. */
export const pickFolder = () => invoke<string | null>('pick_folder');

/** Calls `cb` with the camera's id for every camera event except `Frame` (poll `liveViewFrame`). */
export const onCameraEvent = (cb: (cameraId: CameraId, event: CameraEvent) => void): Promise<UnlistenFn> =>
  listen<TaggedEvent>('camera-event', (e) => cb(e.payload.camera_id, e.payload.event));
