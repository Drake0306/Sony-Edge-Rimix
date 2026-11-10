// Wires the store to the camera events and swaps between the connect screen
// and the main screen. Also owns toasts and the single-key shortcuts.

import { createActions, type Actions } from './actions';
import * as connectScreen from './components/connectScreen';
import * as dock from './components/dock';
import * as filmstrip from './components/filmstrip';
import * as liveview from './components/liveview';
import * as sidePanels from './components/sidePanels';
import * as topbar from './components/topbar';
import { el, isTyping } from './dom';
import { getSettings, getState, onCameraEvent, type CameraEvent } from './ipc';
import { applyEvent, createStore, initialState, watch, type AppStore } from './state';

const TOAST_MS = 4000;

type Mount = (root: HTMLElement, store: AppStore, actions: Actions) => () => void;

const MAIN: Mount[] = [liveview.mount, topbar.mount, sidePanels.mount, filmstrip.mount, dock.mount];

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

export async function startApp(root: HTMLElement): Promise<void> {
  const store = createStore(initialState());
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

  document.addEventListener('keydown', (e) => {
    if (store.get().screen !== 'main' || isTyping(e.target) || e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key === ' ') actions.shoot();
    else if (e.key === 'r' || e.key === 'R') actions.toggleRecord();
    else if (e.key === 'f' || e.key === 'F') actions.focus();
    else return;
    e.preventDefault();
  });

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

  try {
    const [settings, camera] = await Promise.all([getSettings(), getState()]);
    store.set({ settings });
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
