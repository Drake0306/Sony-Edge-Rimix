// A tiny observable store and the reducer that folds camera events into the
// app state. Components read with `get()` and re-render on `subscribe`.

import { sshErrorMessage } from './copy';
import type { IntervalRun } from './interval';
import type {
  AppSettings,
  CameraEvent,
  CameraState,
  CardMode,
  CardObject,
  DiscoveredCamera,
  FeedStatus,
  Found,
  SftpStatus,
} from './ipc';

export interface Store<T> {
  get(): T;
  set(partial: Partial<T>): void;
  /** Calls `fn` after every `set`; returns a function that stops it. */
  subscribe(fn: (state: T) => void): () => void;
}

export function createStore<T extends object>(initial: T): Store<T> {
  let state = initial;
  const subs = new Set<(state: T) => void>();
  return {
    get: () => state,
    set(partial) {
      state = { ...state, ...partial };
      for (const fn of [...subs]) fn(state);
    },
    subscribe(fn) {
      subs.add(fn);
      return () => subs.delete(fn);
    },
  };
}

export interface CaptureItem {
  path: string;
  filename: string;
  bytes: number;
  /** The camera that took it; null when unknown. */
  cameraId: string | null;
  /** That camera's model, shown on the tile while several are connected. */
  cameraLabel: string;
}

export interface Transfer {
  received: number;
  total: number;
}

/** The active camera's card as the card browser shows it. */
export interface CardState {
  items: CardObject[];
  loading: boolean;
  /** Why the card could not be listed. */
  error: string | null;
  /** How the camera lets its card be browsed; null until known. */
  mode: CardMode | null;
  /** Files on their way, by handle. */
  downloads: Record<number, Transfer>;
  /** Where each downloaded file was saved, by handle. */
  done: Record<number, string>;
  /** Why a file's last download failed, by handle. */
  failed: Record<number, string>;
}

export const emptyCard = (): CardState => ({
  items: [],
  loading: false,
  error: null,
  mode: null,
  downloads: {},
  done: {},
  failed: {},
});

/** The last shot that could not be received or saved. */
export interface SaveError {
  filename: string;
  error: string;
}

export interface AppState {
  screen: 'connect' | 'main';
  /** The active camera's state: always `cameras[activeId]` once it is known. */
  camera: CameraState | null;
  /** Every connected camera by id, in the order they were connected. */
  cameras: Record<string, CameraState>;
  /** The camera the controls, live view and shortcuts act on. */
  activeId: string | null;
  /** True while the connect screen is up to add a camera beside the connected ones. */
  keepCameras: boolean;
  settings: AppSettings | null;
  /** True while `settings` are defaults standing in for settings that failed to load and have not been saved since. */
  unsaved: boolean;
  nearby: DiscoveredCamera[];
  /** Cameras heard over Bluetooth, listed under `nearby`. */
  bleNearby: Found[];
  /** Why reading a camera's Wi-Fi details over Bluetooth failed, by address, until the list changes. */
  bleFailed: Record<string, string>;
  /** True once Bluetooth turned out to be off or not allowed; the connect screen stops scanning. */
  bleUnavailable: boolean;
  /** The system refused the last search for cameras: see `ALLOW_LOCAL_NETWORK`. */
  localNetworkBlocked: boolean;
  /** Newest first, at most six. */
  captures: CaptureItem[];
  /** Files on their way from a camera, keyed by `${cameraId}/${camera filename}`. */
  transfers: Record<string, Transfer>;
  /** Shown in Save to until the next shot is saved. */
  saveError: SaveError | null;
  status: { fps: number; lastShotAt: number | null };
  /** Live view overlays switched from the Scopes panel. */
  overlays: { thirds: boolean; zebra: boolean; waveform: boolean; falseColour: boolean };
  /** The active camera's reconnect attempt while its link is down, otherwise null. */
  reconnecting: number | null;
  /** Reconnect attempts of every camera whose link is down, by id. */
  attempts: Record<string, number>;
  /** Progress of a connect started from the connect screen, otherwise null. */
  connecting: { phase: 'joining' | 'finding' | 'connecting'; text: string } | null;
  error: string | null;
  /** The preset slot being applied, otherwise null. */
  presetBusy: number | null;
  /** The slot last applied without failures, until another is applied or the camera disconnects. */
  activePreset: number | null;
  /**
   * A camera's SSH fingerprint waiting for the user to compare it with the
   * camera's screen; `retry` runs the same connect again with it confirmed.
   */
  pendingFingerprint: PendingFingerprint | null;
  /** A remembered Access Authentication camera whose password the connect screen asks for. */
  sshPrefill: SshPrefill | null;
  /** The running interval timer, otherwise null. */
  interval: IntervalRun | null;
  /** The active camera's card; it belongs to that camera and goes when another becomes active. */
  card: CardState;
  /** True while the card browser is open. */
  cardOpen: boolean;
  /** Receive from camera, as last read; null until the settings popover asks. */
  sftp: SftpStatus | null;
  /** False when `RIMIX_WEBCAM=off`: the connect screen leaves the webcam alone. */
  webcam: boolean;
  /** Webcam mode: the camera's picture served to other apps; null until first read. */
  feed: FeedStatus | null;
}

export interface PendingFingerprint {
  sha256: string;
  md5: string;
  retry: () => Promise<boolean>;
  /**
   * Set when the camera's fingerprint differs from the confirmed one: there
   * is nothing to confirm, only both fingerprints to show.
   */
  changed?: { expected: string; actual: string };
}

/** Where the connect screen fills in a remembered camera's login (no password is kept). */
export interface SshPrefill {
  user: string;
  ssid: string | null;
  ip: string;
}

export type AppStore = Store<AppState>;

export const MAX_CAPTURES = 6;

export const initialState = (): AppState => ({
  screen: 'connect',
  camera: null,
  cameras: {},
  activeId: null,
  keepCameras: false,
  settings: null,
  unsaved: false,
  nearby: [],
  bleNearby: [],
  bleFailed: {},
  bleUnavailable: false,
  localNetworkBlocked: false,
  captures: [],
  transfers: {},
  saveError: null,
  status: { fps: 0, lastShotAt: null },
  overlays: { thirds: true, zebra: false, waveform: false, falseColour: false },
  reconnecting: null,
  attempts: {},
  connecting: null,
  error: null,
  presetBusy: null,
  activePreset: null,
  pendingFingerprint: null,
  sshPrefill: null,
  interval: null,
  card: emptyCard(),
  cardOpen: false,
  sftp: null,
  webcam: true,
  feed: null,
});

/** What the app uses when the settings file cannot be read. */
export const defaultSettings = (destinationBase = ''): AppSettings => ({
  appearance: 'Dark',
  layout: 'Dock',
  destinationMode: 'FolderPerDate',
  destinationBase,
  recentCameras: [],
  lastIp: null,
  wifiJoin: 'Automatic',
  webcamId: null,
  presets: [],
  bluetooth: true,
});

/**
 * The store after a Bluetooth scan heard `found`: failures are forgotten
 * when the cameras heard are not the same ones as before.
 */
export function bleListPatch(state: AppState, found: Found[]): Partial<AppState> {
  const key = (list: Found[]) => list.map((f) => f.address).sort().join('\n');
  const same = key(state.bleNearby) === key(found);
  return same ? { bleNearby: found } : { bleNearby: found, bleFailed: {} };
}

/**
 * Calls `fn` now and whenever `select` returns something new (compared by
 * identity), so a component re-renders only for the slice it shows.
 */
export function watch<T extends object, S>(
  store: Store<T>,
  select: (state: T) => S,
  fn: (slice: S, state: T) => void,
): () => void {
  let last = select(store.get());
  fn(last, store.get());
  return store.subscribe((state) => {
    const next = select(state);
    if (next === last) return;
    last = next;
    fn(next, state);
  });
}

/** The status line's numbers for a camera. */
export const statusOf = (camera: CameraState): AppState['status'] => ({
  fps: camera.live_view_fps,
  lastShotAt: camera.last_shot_at,
});

/**
 * The fields that show `id` as the active camera. Another camera's card
 * goes, and the card browser closes with it.
 */
export function activate(state: AppState, id: string | null): Partial<AppState> {
  const camera = id === null ? null : (state.cameras[id] ?? null);
  return {
    activeId: id,
    camera,
    reconnecting: id === null ? null : (state.attempts[id] ?? null),
    ...(camera ? { status: statusOf(camera) } : {}),
    ...(id !== state.activeId ? { card: emptyCard(), cardOpen: false } : {}),
  };
}

/** `record` without `key`. */
export function without<T>(record: Record<string, T>, key: string): Record<string, T> {
  const rest = { ...record };
  delete rest[key];
  return rest;
}

/** How many cameras are connected. */
export const cameraCount = (state: AppState) => Object.keys(state.cameras).length;

const transferKey = (cameraId: string | null, filename: string) =>
  cameraId === null ? filename : `${cameraId}/${filename}`;

/** The camera's filename in a `transfers` key. */
export const transferName = (key: string) => key.slice(key.lastIndexOf('/') + 1);

/** The camera id in a `transfers` key, or null for one without. */
export const transferCamera = (key: string) => (key.includes('/') ? key.slice(0, key.lastIndexOf('/')) : null);

/** `record` without the numeric `key`. */
export function withoutHandle<T>(record: Record<number, T>, key: number): Record<number, T> {
  const rest = { ...record };
  delete rest[key];
  return rest;
}

/**
 * What an event from the active camera changes in its card: progress for a
 * file being downloaded, or its failure, shown on its row. Only events for
 * a handle still downloading count, so one that overtook the
 * `card_download` reply cannot bring a settled row back. A new session
 * (`Connected`) empties the list and the per-file results, since PTP
 * handles only hold within one session; the open panel lists the card
 * again. Null when nothing changes.
 */
function cardEvent(card: CardState, event: CameraEvent): CardState | null {
  if (event === 'Connected') {
    const empty = card.items.length === 0 && isEmpty(card.done) && isEmpty(card.failed);
    return empty ? null : { ...card, items: [], done: {}, failed: {} };
  }
  if (typeof event !== 'object') return null;
  if ('CardDownloadProgress' in event) {
    const { handle, received, total } = event.CardDownloadProgress;
    if (!(handle in card.downloads)) return null;
    return { ...card, downloads: { ...card.downloads, [handle]: { received, total } } };
  }
  if ('CardDownloadFailed' in event) {
    const { handle, error } = event.CardDownloadFailed;
    if (!(handle in card.downloads)) return null;
    return { ...card, downloads: withoutHandle(card.downloads, handle), failed: { ...card.failed, [handle]: error } };
  }
  return null;
}

const isEmpty = (record: Record<number, unknown>) => Object.keys(record).length === 0;

/**
 * What a camera event from `cameraId` changes; the toast for a skipped shot
 * is the app's job. Events from a camera that is not connected (late ones
 * from a camera just disconnected) change nothing. Without an id the event
 * is the active camera's.
 */
export function applyEvent(state: AppState, event: CameraEvent, cameraId: string | null = state.activeId): Partial<AppState> {
  if (cameraId === SFTP_CAMERA_ID) return receivedPatch(state, event);
  if (cameraId !== null && !(cameraId in state.cameras)) return {};
  const patch = cameraPatch(state, event, cameraId);
  const card = cameraId === state.activeId ? cardEvent(state.card, event) : null;
  return card ? { ...patch, card } : patch;
}

/** The camera id files the camera pushed over SFTP are tagged with. */
export const SFTP_CAMERA_ID = 'sftp';

/** The label on tiles of files the camera pushed over SFTP. */
export const RECEIVED_LABEL = 'Received';

/**
 * A file the camera pushed over SFTP joins the captures; it belongs to no
 * connected camera, so nothing else changes.
 */
function receivedPatch(state: AppState, event: CameraEvent): Partial<AppState> {
  if (typeof event !== 'object' || !('CaptureSaved' in event)) return {};
  const { path, filename, bytes } = event.CaptureSaved;
  const item: CaptureItem = { path, filename, bytes, cameraId: SFTP_CAMERA_ID, cameraLabel: RECEIVED_LABEL };
  const captures = [item, ...state.captures.filter((c) => c.path !== path)];
  return { captures: captures.slice(0, MAX_CAPTURES) };
}

/** What `applyEvent` changes apart from the card. */
function cameraPatch(state: AppState, event: CameraEvent, cameraId: string | null): Partial<AppState> {
  const active = cameraId === state.activeId;
  if (event === 'Connected') {
    if (cameraId === null || !(cameraId in state.attempts)) {
      return active && state.reconnecting !== null ? { reconnecting: null } : {};
    }
    return { attempts: without(state.attempts, cameraId), ...(active ? { reconnecting: null } : {}) };
  }
  if (event === 'Frame') return {};
  if ('StateChanged' in event) {
    const camera = event.StateChanged;
    const cameras = cameraId === null ? {} : { cameras: { ...state.cameras, [cameraId]: camera } };
    if (!active) return cameras;
    return { ...cameras, screen: 'main', camera, status: statusOf(camera) };
  }
  if ('Reconnecting' in event) {
    const attempt = event.Reconnecting.attempt;
    const attempts = cameraId === null ? {} : { attempts: { ...state.attempts, [cameraId]: attempt } };
    return { ...attempts, ...(active ? { reconnecting: attempt } : {}) };
  }
  if ('Disconnected' in event) {
    // A plain link loss keeps the interval timer: its ticks count as missed
    // until the camera is back. Only a final failure, where reconnecting has
    // stopped, ends the run on that camera.
    // The camera refused the login or its fingerprint changed: reconnecting
    // has stopped, so say why and leave the camera shown as disconnected.
    const error = sshErrorMessage(event.Disconnected.reason);
    if (!error) return {};
    const patch: Partial<AppState> = { error };
    const runHere = state.interval !== null && (state.interval.cameraId ?? state.activeId) === cameraId;
    if (runHere) patch.interval = null;
    if (cameraId !== null) {
      const gone = { ...state.cameras[cameraId], connected: false };
      patch.cameras = { ...state.cameras, [cameraId]: gone };
      patch.attempts = without(state.attempts, cameraId);
    }
    if (active) {
      patch.reconnecting = null;
      patch.camera = state.camera && { ...state.camera, connected: false };
    }
    return patch;
  }
  if ('CaptureProgress' in event) {
    const { filename, received, total } = event.CaptureProgress;
    return { transfers: { ...state.transfers, [transferKey(cameraId, filename)]: { received, total } } };
  }
  if ('CaptureSaved' in event) {
    const { path, filename, bytes, camera_filename } = event.CaptureSaved;
    const transfers = without(state.transfers, transferKey(cameraId, camera_filename));
    const cameraLabel = (cameraId === null ? state.camera : state.cameras[cameraId])?.model ?? '';
    const item: CaptureItem = { path, filename, bytes, cameraId, cameraLabel };
    const captures = [item, ...state.captures.filter((c) => c.path !== path)];
    return { captures: captures.slice(0, MAX_CAPTURES), transfers, saveError: null };
  }
  if ('CaptureFailed' in event) {
    const { filename, error } = event.CaptureFailed;
    const key = transferKey(cameraId, filename);
    const patch: Partial<AppState> = {};
    if (key in state.transfers) patch.transfers = without(state.transfers, key);
    const same = state.saveError?.filename === filename && state.saveError.error === error;
    if (!same) patch.saveError = { filename, error };
    return patch;
  }
  if ('CaptureSkipped' in event) {
    const key = transferKey(cameraId, event.CaptureSkipped.filename);
    return key in state.transfers ? { transfers: without(state.transfers, key) } : {};
  }
  return {};
}

/**
 * Whether an interval shot on camera `id` (the active one by default) would
 * collide with something: it is recording, one of its files is still on
 * its way, or its link is down.
 */
export function cameraBusy(state: AppState, id: string | null = state.activeId): boolean {
  const active = id === state.activeId;
  const camera = id !== null && id in state.cameras ? state.cameras[id] : active ? state.camera : null;
  const down = active ? state.reconnecting !== null : id !== null && id in state.attempts;
  if (!camera?.connected || down) return true;
  const own = (key: string) => id === null || transferCamera(key) === id || transferCamera(key) === null;
  return camera.recording || Object.keys(state.transfers).some(own);
}
