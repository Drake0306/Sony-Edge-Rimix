import { describe, expect, test, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import cameraState from './fixtures/camera-state.json';
import type { Actions } from './actions';
import { eventToast, intervalTicker, startApp, toastConnecting } from './app';
import type { CameraState } from './ipc';
import { start } from './interval';
import { createStore, initialState, type AppState } from './state';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
// The live view polls frames and draws on a canvas, which jsdom lacks.
vi.mock('./components/liveview', () => ({ mount: () => () => {} }));

describe('startApp', () => {
  test('settings that fail to load fall back to unsaved defaults the user can still change', async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === 'get_settings') return Promise.reject(new Error('disk on fire'));
      if (cmd === 'get_state') return Promise.resolve(cameraState);
      return Promise.resolve();
    });
    document.body.innerHTML = '';
    const root = document.createElement('div');
    document.body.append(root);
    await startApp(root);

    const html = document.documentElement;
    expect(html.getAttribute('data-theme')).toBe('dark');
    expect(html.getAttribute('data-layout')).toBe('dock');
    expect(root.querySelector('.dockwrap')).not.toBeNull();
    expect(root.querySelector('.toast')!.textContent).toContain('disk on fire');

    const radios = (label: string) => [...root.querySelectorAll<HTMLButtonElement>(`.apppop [aria-label="${label}"] button`)];
    radios('Controls layout')[1].click();
    expect(html.getAttribute('data-layout')).toBe('ring');
    expect(root.querySelector('.ring')).not.toBeNull();
    expect(root.querySelector('.dockwrap')).toBeNull();
    expect(radios('Controls layout')[1].getAttribute('aria-checked')).toBe('true');
    expect(invoke).toHaveBeenCalledWith('save_settings', {
      settings: {
        appearance: 'Dark',
        layout: 'Ring',
        destinationMode: 'FolderPerDate',
        destinationBase: '',
        recentCameras: [],
        lastIp: null,
        wifiJoin: 'Automatic',
        webcamId: null,
        presets: [],
      },
    });

    radios('Theme')[1].click();
    expect(html.getAttribute('data-theme')).toBe('light');
    expect(radios('Theme')[1].getAttribute('aria-checked')).toBe('true');
  });
});

describe('event toasts', () => {
  test('a failed save is shown in Save to, not as a toast; a skipped repeat still toasts', () => {
    expect(eventToast({ CaptureFailed: { filename: 'DSC00001.JPG', error: 'Cannot save to /x: denied' } })).toBeNull();
    expect(eventToast({ CaptureSkipped: { filename: 'DSC00001.JPG' } })).toContain('DSC00001.JPG');
  });
});

describe('connect screen', () => {
  test('App settings is there before the first connection', async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === 'get_settings') return Promise.reject(new Error('none'));
      if (cmd === 'get_state') return Promise.resolve(null);
      if (cmd === 'discover') return Promise.resolve([]);
      return Promise.resolve();
    });
    document.body.innerHTML = '';
    const root = document.createElement('div');
    document.body.append(root);
    await startApp(root);
    expect(root.querySelector('.screen.connect')).not.toBeNull();
    const gear = root.querySelector<HTMLButtonElement>('.screen.connect .gear')!;
    expect(gear).not.toBeNull();
    const pop = root.querySelector<HTMLElement>('.screen.connect .apppop')!;
    expect(pop.hidden).toBe(true);
    gear.click();
    expect(pop.hidden).toBe(false);
    expect(pop.querySelector('[aria-label="Wi-Fi join"]')).not.toBeNull();
    expect(pop.querySelector('select[aria-label="Webcam"]')).not.toBeNull();
  });
});

describe('connect progress on the main screen', () => {
  test('each new progress line is a toast on the main screen only', () => {
    const store = createStore<AppState>({ ...initialState(), screen: 'main' });
    const toast = vi.fn();
    toastConnecting(store, toast);
    store.set({ connecting: { phase: 'joining', text: 'Joining X' } });
    store.set({ connecting: { phase: 'joining', text: 'Joining X' } });
    store.set({ connecting: { phase: 'finding', text: 'Looking for the camera on X' } });
    store.set({ connecting: null });
    expect(toast.mock.calls).toEqual([['Joining X'], ['Looking for the camera on X']]);

    store.set({ screen: 'connect' });
    store.set({ connecting: { phase: 'joining', text: 'Joining Y' } });
    expect(toast).toHaveBeenCalledTimes(2);
  });
});

describe('interval ticker', () => {
  const camera = { ...(cameraState as unknown as CameraState), connected: true, recording: false };

  function setup(state: Partial<AppState> = {}) {
    const store = createStore<AppState>({ ...initialState(), screen: 'main', camera, ...state });
    const actions = { shoot: vi.fn() } as unknown as Actions;
    const stop = intervalTicker(store, actions);
    return { store, actions, stop };
  }

  test('shoots once after one period with a connected camera, then stops at its count', () => {
    vi.useFakeTimers();
    try {
      const { store, actions, stop } = setup();
      store.set({ interval: start({ everyS: 5, count: 2 }, Date.now()) });
      vi.advanceTimersByTime(4999);
      expect(actions.shoot).not.toHaveBeenCalled();
      vi.advanceTimersByTime(1);
      expect(actions.shoot).toHaveBeenCalledTimes(1);
      expect(store.get().interval).toMatchObject({ taken: 1, running: true });
      vi.advanceTimersByTime(5000);
      expect(actions.shoot).toHaveBeenCalledTimes(2);
      // The finished run is cleared, so the row reads Off again.
      expect(store.get().interval).toBeNull();
      vi.advanceTimersByTime(20000);
      expect(actions.shoot).toHaveBeenCalledTimes(2);
      stop();
    } finally {
      vi.useRealTimers();
    }
  });

  test('a recording camera or a transfer in flight misses the shot', () => {
    vi.useFakeTimers();
    try {
      const { store, actions, stop } = setup({ camera: { ...camera, recording: true } });
      store.set({ interval: start({ everyS: 2, count: null }, Date.now()) });
      vi.advanceTimersByTime(2000);
      store.set({ camera, transfers: { 'DSC00001.JPG': { received: 1, total: 3 } } });
      vi.advanceTimersByTime(2000);
      expect(actions.shoot).not.toHaveBeenCalled();
      expect(store.get().interval).toMatchObject({ taken: 0, missed: 2 });
      store.set({ transfers: {} });
      vi.advanceTimersByTime(2000);
      expect(actions.shoot).toHaveBeenCalledTimes(1);
      stop();
    } finally {
      vi.useRealTimers();
    }
  });

  test('stopping the run stops the shots', () => {
    vi.useFakeTimers();
    try {
      const { store, actions, stop } = setup();
      store.set({ interval: start({ everyS: 2, count: null }, Date.now()) });
      store.set({ interval: null });
      vi.advanceTimersByTime(10000);
      expect(actions.shoot).not.toHaveBeenCalled();
      stop();
    } finally {
      vi.useRealTimers();
    }
  });
});
