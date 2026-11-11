// Wires the store to the camera events and swaps between the connect screen
// and the main screen, with the dock or the ring as its controls. Also owns
// toasts, the single-key shortcuts, the interval timer's ticker and applying
// the saved theme and layout.

import { createActions, type Actions } from './actions';
import * as appSettings from './components/appSettings';
import * as cardBrowser from './components/cardBrowser';
import * as connectScreen from './components/connectScreen';
import * as dock from './components/dock';
import * as filesScreen from './components/filesScreen';
import * as filmstrip from './components/filmstrip';
import * as liveview from './components/liveview';
import * as ring from './components/ring';
import * as sidePanels from './components/sidePanels';
import * as topbar from './components/topbar';
import * as webcamMode from './components/webcamMode';
import { MOVIES_STAY_ON_CARD } from './copy';
import { el, isTyping } from './dom';
import { tick } from './interval';
import { devFlags, getSettings, getState, listCameras, onCameraEvent, onFilesProgress, type CameraEvent, type CameraState } from './ipc';
import {
  applyEvent,
  cameraBusy,
  createStore,
  statusOf,
  defaultSettings,
  initialState,
  watch,
  type AppState,
  type AppStore,
} from './state';
import { applyAppearance, applyLayout } from './theme';

const TOAST_MS = 4000;
/** How often the interval timer checks whether a shot is due. */
export const INTERVAL_TICK_MS = 250;

type Mount = (root: HTMLElement, store: AppStore, actions: Actions) => () => void;

/** The dock or the ring, whichever the layout setting names; swapped when it changes. */
export const controls: Mount = (root, store, actions) => {
  let unmount = () => {};
  const off = watch(store, (s) => s.settings?.layout === 'Ring', (isRing) => {
    unmount();
    unmount = (isRing ? ring.mount : dock.mount)(root, store, actions);
  });
  return () => {
    off();
    unmount();
  };
};

// App settings mounts before the card browser and the controls so its
// Escape handler runs first; the card browser's runs before the controls'.
const MAIN: Mount[] = [
  liveview.mount,
  webcamMode.mount,
  topbar.mount,
  appSettings.mount,
  sidePanels.mount,
  filmstrip.mount,
  cardBrowser.mount,
  controls,
];
// App settings here too, so the webcam and Wi-Fi join can be set before the first connection.
const CONNECT: Mount[] = [connectScreen.mount, appSettings.mount];
// The files of a camera in Send to Smartphone: no live view, no controls.
// Its own bar has the one setting that matters here, where imports go.
const FILES: Mount[] = [filesScreen.mount];

function toaster(host: HTMLElement): (message: string) => void {
  return (message) => {
    const t = el('div', 'toast glass capsule', message);
    t.setAttribute('role', 'status');
    host.append(t);
    window.setTimeout(() => t.remove(), TOAST_MS);
  };
}

/**
 * Toasts for camera events that are news rather than state. A failed save
 * is not one: it stays in the Save to panel until the next save.
 */
export function eventToast(event: CameraEvent): string | null {
  if (typeof event !== 'object') return null;
  if ('CaptureSkipped' in event) return `Skipped a repeat of ${event.CaptureSkipped.filename}`;
  if ('MovieTransferUnavailable' in event) return MOVIES_STAY_ON_CARD;
  return null;
}

/**
 * On the main screen, each new progress line of a connect (from the
 * Cameras panel, say) is a toast; the connect screen shows its own.
 */
export function toastConnecting(store: AppStore, toast: (message: string) => void): () => void {
  return watch(store, (s) => s.connecting?.text ?? null, (text, s) => {
    if (text && s.screen === 'main') toast(text);
  });
}

/**
 * Folds one camera's event into the store, toasting what is news. On the
 * connect screen, `connect` sets the camera: state from the connected
 * cameras still lands while one is being added, but never flips back to
 * the main screen; after a disconnect, late state is dropped.
 */
export function routeCameraEvent(
  store: AppStore,
  cameraId: string,
  event: CameraEvent,
  toast: (message: string) => void,
): void {
  const msg = eventToast(event);
  if (msg) toast(msg);
  const state = store.get();
  const patch = applyEvent(state, event, cameraId);
  if (state.screen === 'connect') {
    delete patch.screen;
    if (!state.keepCameras) {
      delete patch.camera;
      delete patch.cameras;
      delete patch.status;
    }
  }
  if (Object.keys(patch).length) store.set(patch);
}

/** Space shoots (unless a button has focus), R records, F focuses: once per press, never while typing. */
export function handleShortcut(e: KeyboardEvent, store: AppStore, actions: Actions): void {
  if (e.repeat || store.get().screen !== 'main' || isTyping(e.target)) return;
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  // Space on a focused button presses that button (Stop, say), not the shutter.
  if (e.key === ' ' && e.target instanceof HTMLButtonElement) return;
  if (e.key === ' ') actions.shoot();
  else if (e.key === 'r' || e.key === 'R') actions.toggleRecord();
  else if (e.key === 'f' || e.key === 'F') actions.focus();
  else return;
  e.preventDefault();
}

/**
 * Runs the interval timer while `interval` is running: shoots the run's
 * camera (whichever is active) when a shot is due and that camera is free,
 * and clears the run once it has finished.
 */
export function intervalTicker(store: AppStore, actions: Actions): () => void {
  let timer: number | null = null;
  const step = () => {
    const state = store.get();
    if (!state.interval?.running) return;
    const id = state.interval.cameraId ?? state.activeId;
    const { run, shoot } = tick(state.interval, Date.now(), cameraBusy(state, id));
    if (run !== state.interval) store.set({ interval: run.running ? run : null });
    if (shoot) actions.shoot(state.interval.cameraId);
  };
  const off = watch(store, (s) => !!s.interval?.running, (running) => {
    if (running && timer === null) timer = window.setInterval(step, INTERVAL_TICK_MS);
    else if (!running && timer !== null) {
      window.clearInterval(timer);
      timer = null;
    }
  });
  return () => {
    off();
    if (timer !== null) window.clearInterval(timer);
  };
}

export async function startApp(root: HTMLElement): Promise<void> {
  // Settings that fail to load are replaced by unsaved defaults, so every
  // change still flows through the store; saving is tried on each change.
  let settingsError: unknown = null;
  const settings = await getSettings().catch((e: unknown) => {
    settingsError = e;
    return defaultSettings();
  });
  // Only an explicit `false` turns the webcam off.
  const webcam = (await devFlags().catch(() => null))?.webcam !== false;
  const store: AppStore = createStore<AppState>({ ...initialState(), settings, unsaved: settingsError !== null, webcam });
  // Theme and layout go on <html> before anything renders (Dark and Dock
  // when settings fail to load) and follow the settings from then on.
  watch(store, (s) => s.settings?.appearance ?? 'Dark', (a) => applyAppearance(a));
  watch(store, (s) => s.settings?.layout ?? 'Dock', (l) => applyLayout(l));
  const toasts = el('div', 'toasts');
  const toast = toaster(toasts);
  const actions = createActions(store, toast);
  const screen = el('section', 'screen');
  root.append(screen, toasts);

  let unmount: (() => void)[] = [];
  watch(store, (s) => s.screen, (name) => {
    if (name === 'main' && store.get().keepCameras) store.set({ keepCameras: false });
    unmount.forEach((u) => u());
    screen.className = `screen ${name}`;
    screen.setAttribute('aria-label', name === 'main' ? 'Main screen' : name === 'files' ? 'Files on the camera' : 'Connect screen');
    unmount = (name === 'main' ? MAIN : name === 'files' ? FILES : CONNECT).map((m) => m(screen, store, actions));
  });

  toastConnecting(store, toast);
  intervalTicker(store, actions);

  // The connect screen shows its own errors; on the main screen they are toasts.
  watch(store, (s) => s.error, (error, s) => {
    if (error && s.screen === 'main') toast(error);
  });

  document.addEventListener('keydown', (e) => handleShortcut(e, store, actions));

  await onCameraEvent((cameraId, event) => routeCameraEvent(store, cameraId, event, toast));
  // How far an import from the files screen has come.
  await onFilesProgress(({ id, received, total }) => {
    const files = store.get().files;
    if (files && id in files.progress) store.set({ files: { ...files, progress: { ...files.progress, [id]: { received, total } } } });
  });

  if (settingsError) toast(`Cannot load settings: ${String(settingsError)}`);
  try {
    // After a reload of the window, the cameras may still be connected.
    const camera = await getState();
    if (camera) {
      const list = (await listCameras().catch(() => null)) ?? [];
      const cameras: Record<string, CameraState> = {};
      for (const c of list) {
        const s = c.active ? camera : await getState(c.id).catch(() => null);
        if (s) cameras[c.id] = s;
      }
      const activeId = list.find((c) => c.active)?.id ?? null;
      store.set({ cameras, activeId, camera, screen: 'main', status: statusOf(camera) });
    }
  } catch (e) {
    toast(`Cannot reach the app: ${String(e)}`);
  }
}
