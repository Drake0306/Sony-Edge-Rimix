// Wires the store to the camera events and swaps between the connect screen
// and the main screen, with the dock or the ring as its controls. Also owns
// toasts, the single-key shortcuts and applying the saved theme and layout.

import { createActions, type Actions } from './actions';
import * as appSettings from './components/appSettings';
import * as connectScreen from './components/connectScreen';
import * as dock from './components/dock';
import * as filmstrip from './components/filmstrip';
import * as liveview from './components/liveview';
import * as ring from './components/ring';
import * as sidePanels from './components/sidePanels';
import * as topbar from './components/topbar';
import { el, isTyping } from './dom';
import { getSettings, getState, onCameraEvent, type CameraEvent } from './ipc';
import { applyEvent, createStore, defaultSettings, initialState, watch, type AppState, type AppStore } from './state';
import { applyAppearance, applyLayout } from './theme';

const TOAST_MS = 4000;

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

// App settings mounts before the controls so its Escape handler runs first.
const MAIN: Mount[] = [liveview.mount, topbar.mount, appSettings.mount, sidePanels.mount, filmstrip.mount, controls];

function toaster(host: HTMLElement): (message: string) => void {
  return (message) => {
    const t = el('div', 'toast glass capsule', message);
    t.setAttribute('role', 'status');
    host.append(t);
    window.setTimeout(() => t.remove(), TOAST_MS);
  };
}

/** Toasts for camera events that are news rather than state. */
function eventToast(event: CameraEvent): string | null {
  if (typeof event !== 'object') return null;
  if ('CaptureSkipped' in event) return `Skipped a repeat of ${event.CaptureSkipped.filename}`;
  if ('CaptureFailed' in event) return `Could not save ${event.CaptureFailed.filename}: ${event.CaptureFailed.error}`;
  return null;
}

/** Space shoots, R records, F focuses: once per press, never while typing. */
export function handleShortcut(e: KeyboardEvent, store: AppStore, actions: Actions): void {
  if (e.repeat || store.get().screen !== 'main' || isTyping(e.target)) return;
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  if (e.key === ' ') actions.shoot();
  else if (e.key === 'r' || e.key === 'R') actions.toggleRecord();
  else if (e.key === 'f' || e.key === 'F') actions.focus();
  else return;
  e.preventDefault();
}

export async function startApp(root: HTMLElement): Promise<void> {
  // Settings that fail to load are replaced by unsaved defaults, so every
  // change still flows through the store; saving is tried on each change.
  let settingsError: unknown = null;
  const settings = await getSettings().catch((e: unknown) => {
    settingsError = e;
    return defaultSettings();
  });
  const store: AppStore = createStore<AppState>({ ...initialState(), settings, unsaved: settingsError !== null });
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
    unmount.forEach((u) => u());
    screen.className = `screen ${name}`;
    screen.setAttribute('aria-label', name === 'main' ? 'Main screen' : 'Connect screen');
    unmount = (name === 'main' ? MAIN : [connectScreen.mount]).map((m) => m(screen, store, actions));
  });

  // The connect screen shows its own errors; on the main screen they are toasts.
  watch(store, (s) => s.error, (error, s) => {
    if (error && s.screen === 'main') toast(error);
  });

  document.addEventListener('keydown', (e) => handleShortcut(e, store, actions));

  await onCameraEvent((event) => {
    const msg = eventToast(event);
    if (msg) toast(msg);
    const patch = applyEvent(store.get(), event);
    // On the connect screen, `connect` sets the camera; late state from a
    // camera the user disconnected must not bring the main screen back.
    if (store.get().screen === 'connect') {
      delete patch.screen;
      delete patch.camera;
      delete patch.status;
    }
    if (Object.keys(patch).length) store.set(patch);
  });

  if (settingsError) toast(`Cannot load settings: ${String(settingsError)}`);
  try {
    const camera = await getState();
    if (camera) {
      store.set({
        camera,
        screen: 'main',
        status: { fps: camera.live_view_fps, lastShotAt: camera.last_shot_at },
      });
    }
  } catch (e) {
    toast(`Cannot reach the app: ${String(e)}`);
  }
}
