// A tiny observable store and the reducer that folds camera events into the
// app state. Components read with `get()` and re-render on `subscribe`.

import type { AppSettings, CameraEvent, CameraState, DiscoveredCamera } from './ipc';

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
}

export interface Transfer {
  received: number;
  total: number;
}

/** The last shot that could not be received or saved. */
export interface SaveError {
  filename: string;
  error: string;
}

export interface AppState {
  screen: 'connect' | 'main';
  camera: CameraState | null;
  settings: AppSettings | null;
  /** True while `settings` are defaults standing in for settings that failed to load and have not been saved since. */
  unsaved: boolean;
  nearby: DiscoveredCamera[];
  /** Newest first, at most six. */
  captures: CaptureItem[];
  /** Files on their way from the camera, keyed by the camera's filename. */
  transfers: Record<string, Transfer>;
  /** Shown in Save to until the next shot is saved. */
  saveError: SaveError | null;
  status: { fps: number; lastShotAt: number | null };
  /** Live view overlays switched from the Scopes panel. */
  overlays: { thirds: boolean; zebra: boolean };
  /** Reconnect attempt while the link is down, otherwise null. */
  reconnecting: number | null;
  /** Progress of a connect started from the connect screen, otherwise null. */
  connecting: { phase: 'joining' | 'finding' | 'connecting'; text: string } | null;
  error: string | null;
}

export type AppStore = Store<AppState>;

export const MAX_CAPTURES = 6;

export const initialState = (): AppState => ({
  screen: 'connect',
  camera: null,
  settings: null,
  unsaved: false,
  nearby: [],
  captures: [],
  transfers: {},
  saveError: null,
  status: { fps: 0, lastShotAt: null },
  overlays: { thirds: true, zebra: false },
  reconnecting: null,
  connecting: null,
  error: null,
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
});

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

/** What a camera event changes; the toast for a skipped shot is the app's job. */
export function applyEvent(state: AppState, event: CameraEvent): Partial<AppState> {
  if (event === 'Connected') return state.reconnecting === null ? {} : { reconnecting: null };
  if (event === 'Frame') return {};
  if ('StateChanged' in event) {
    const camera = event.StateChanged;
    return {
      screen: 'main',
      camera,
      status: { fps: camera.live_view_fps, lastShotAt: camera.last_shot_at },
    };
  }
  if ('Reconnecting' in event) return { reconnecting: event.Reconnecting.attempt };
  if ('CaptureProgress' in event) {
    const { filename, received, total } = event.CaptureProgress;
    return { transfers: { ...state.transfers, [filename]: { received, total } } };
  }
  if ('CaptureSaved' in event) {
    const { path, filename, bytes, camera_filename } = event.CaptureSaved;
    const transfers = withoutTransfer(state.transfers, camera_filename);
    const captures = [{ path, filename, bytes }, ...state.captures.filter((c) => c.path !== path)];
    return { captures: captures.slice(0, MAX_CAPTURES), transfers, saveError: null };
  }
  if ('CaptureFailed' in event) {
    const { filename, error } = event.CaptureFailed;
    const patch: Partial<AppState> = {};
    if (filename in state.transfers) patch.transfers = withoutTransfer(state.transfers, filename);
    const same = state.saveError?.filename === filename && state.saveError.error === error;
    if (!same) patch.saveError = { filename, error };
    return patch;
  }
  if ('CaptureSkipped' in event) {
    const { filename } = event.CaptureSkipped;
    return filename in state.transfers ? { transfers: withoutTransfer(state.transfers, filename) } : {};
  }
  return {};
}

function withoutTransfer(transfers: Record<string, Transfer>, filename: string): Record<string, Transfer> {
  const rest = { ...transfers };
  delete rest[filename];
  return rest;
}
