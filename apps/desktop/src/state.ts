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

export interface AppState {
  screen: 'connect' | 'main';
  camera: CameraState | null;
  settings: AppSettings | null;
  nearby: DiscoveredCamera[];
  /** Newest first, at most six. */
  captures: CaptureItem[];
  /** Files on their way from the camera, keyed by filename. */
  transfers: Record<string, Transfer>;
  status: { fps: number; lastShotAt: number | null };
  /** Live view overlays switched from the Scopes panel. */
  overlays: { thirds: boolean; zebra: boolean };
  /** Reconnect attempt while the link is down, otherwise null. */
  reconnecting: number | null;
  error: string | null;
}

export type AppStore = Store<AppState>;

export const MAX_CAPTURES = 6;

export const initialState = (): AppState => ({
  screen: 'connect',
  camera: null,
  settings: null,
  nearby: [],
  captures: [],
  transfers: {},
  status: { fps: 0, lastShotAt: null },
  overlays: { thirds: true, zebra: false },
  reconnecting: null,
  error: null,
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

/** What a camera event changes; toasts for skipped and failed shots are the app's job. */
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
    const { path, filename, bytes } = event.CaptureSaved;
    const transfers = { ...state.transfers };
    delete transfers[filename];
    const captures = [{ path, filename, bytes }, ...state.captures.filter((c) => c.path !== path)];
    return { captures: captures.slice(0, MAX_CAPTURES), transfers };
  }
  if ('CaptureSkipped' in event || 'CaptureFailed' in event) {
    const filename = 'CaptureSkipped' in event ? event.CaptureSkipped.filename : event.CaptureFailed.filename;
    if (!(filename in state.transfers)) return {};
    const transfers = { ...state.transfers };
    delete transfers[filename];
    return { transfers };
  }
  return {};
}
