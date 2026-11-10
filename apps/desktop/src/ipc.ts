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
// and a `PtpValue` as {"U16": 5}. The app's own types (AppSettings and
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
  model: string;
  firmware: string;
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
/** Waits up to `timeoutMs` for a camera to answer on the network. */
export const waitForCamera = (timeoutMs: number) =>
  invoke<DiscoveredCamera>('wait_for_camera', { timeoutMs });
/** `ip` is an IP address (port 15740) or `ip:port`. */
export const connect = (ip: string) => invoke<CameraState>('connect', { ip });
export const disconnect = () => invoke<void>('disconnect');
export const getState = () => invoke<CameraState | null>('get_state');
export const setProperty = (code: number, raw: PtpValue) =>
  invoke<void>('set_property', { code, raw });
export const stepProperty = (code: number, delta: number) =>
  invoke<void>('step_property', { code, delta });
export const shoot = () => invoke<void>('shoot');
export const focus = () => invoke<void>('focus');
export const toggleRecord = () => invoke<void>('toggle_record');
/** The latest live view JPEG; empty when there is no frame yet. */
export const liveViewFrame = () => invoke<ArrayBuffer>('live_view_frame');
export const liveViewMeta = () => invoke<LiveViewMeta>('live_view_meta');
export const getSettings = () => invoke<AppSettings>('get_settings');
export const saveSettings = (settings: AppSettings) =>
  invoke<void>('save_settings', { settings });
/** A saved JPEG's bytes for the filmstrip; rejects for RAW files and files over 25 MB. */
export const readThumbnail = (path: string) => invoke<ArrayBuffer>('read_thumbnail', { path });
/** `null` when the dialog is cancelled. */
export const pickFolder = () => invoke<string | null>('pick_folder');

/** Calls `cb` for every camera event except `Frame` (poll `liveViewFrame`). */
export const onCameraEvent = (cb: (event: CameraEvent) => void): Promise<UnlistenFn> =>
  listen<CameraEvent>('camera-event', (e) => cb(e.payload));
