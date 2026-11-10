import { describe, expect, test, vi } from 'vitest';
import cameraState from '../fixtures/camera-state.json';
import type { Actions } from '../actions';
import type { AppSettings, CameraState, RecentCamera } from '../ipc';
import { createActions } from '../actions';
import { createStore, initialState, type AppState } from '../state';
import { mount, saveToSection } from './sidePanels';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => Promise.resolve()) }));

const settings: AppSettings = {
  appearance: 'Dark',
  layout: 'Dock',
  destinationMode: 'CustomFolder',
  destinationBase: '/Volumes/Card/Shots',
  recentCameras: [],
  lastIp: null,
  wifiJoin: 'Automatic',
  webcamId: null,
  presets: [],
};

describe('Save to', () => {
  test('shows a save failure until it is cleared', () => {
    const store = createStore({ ...initialState(), screen: 'main' as const, settings });
    const section = saveToSection(store, createActions(store));
    const line = () => section.el.querySelector<HTMLElement>('.save-error');
    expect(line()!.hidden).toBe(true);

    store.set({
      saveError: { filename: 'DSC00001.JPG', error: 'Cannot save to /Volumes/Card/Shots: Read-only file system' },
    });
    expect(line()!.hidden).toBe(false);
    expect(line()!.getAttribute('role')).toBe('alert');
    expect(line()!.textContent).toBe('Cannot save to /Volumes/Card/Shots: Read-only file system');

    // A failure that is not about the folder names the file.
    store.set({ saveError: { filename: 'DSC00002.JPG', error: 'operation timed out' } });
    expect(line()!.textContent).toBe('Could not receive DSC00002.JPG: operation timed out');

    store.set({ saveError: null });
    expect(line()!.hidden).toBe(true);
    section.destroy();
  });
});

describe('Cameras', () => {
  test('the current camera, once its link is lost, reconnects from its row', () => {
    const rc: RecentCamera = {
      ip: '192.168.122.1', model: 'ILCE-7M4', lastUsed: 1, ssid: null, sshUser: 'cam', sshFingerprint: 'SHA256:x',
    };
    const camera = { ...(cameraState as unknown as CameraState), model: 'ILCE-7M4', connected: true };
    const store = createStore({
      ...initialState(),
      screen: 'main' as const,
      camera,
      settings: { ...settings, recentCameras: [rc], lastIp: rc.ip },
    });
    const actions = { connectRecent: vi.fn(() => Promise.resolve(false)), disconnect: vi.fn() } as unknown as Actions;
    const root = document.createElement('div');
    const unmount = mount(root, store, actions);
    const row = () => root.querySelector<HTMLButtonElement>('[aria-label="Cameras"] button')!;
    row().click();
    expect(actions.connectRecent).not.toHaveBeenCalled();
    store.set({ camera: { ...camera, connected: false } });
    row().click();
    expect(actions.connectRecent).toHaveBeenCalledWith(rc);
    unmount();
  });

  test('a camera in the middle of reconnecting ignores a click on its row', () => {
    const rc: RecentCamera = { ip: '10.0.0.5', model: 'A7 IV', lastUsed: 1, ssid: null };
    const camera = { ...(cameraState as unknown as CameraState), model: 'A7 IV', connected: false };
    const store = createStore<AppState>({
      ...initialState(),
      screen: 'main',
      camera,
      reconnecting: 2,
      settings: { ...settings, recentCameras: [rc], lastIp: rc.ip },
    });
    const actions = { connectRecent: vi.fn(() => Promise.resolve(true)), disconnect: vi.fn() } as unknown as Actions;
    const root = document.createElement('div');
    const unmount = mount(root, store, actions);
    const row = () => root.querySelector<HTMLButtonElement>('[aria-label="Cameras"] button')!;
    row().click();
    expect(actions.connectRecent).not.toHaveBeenCalled();
    // Once reconnecting has given up, the row connects again.
    store.set({ reconnecting: null });
    row().click();
    expect(actions.connectRecent).toHaveBeenCalledWith(rc);
    unmount();
  });
});

describe('Cameras with a shared address', () => {
  const zv: RecentCamera = { ip: '192.168.122.1:15740', model: 'ZV-E10', lastUsed: 2, ssid: 'DIRECT-abc1:ZV-E10' };
  const a7: RecentCamera = { ip: '192.168.122.1:15740', model: 'ILCE-7M4', lastUsed: 1, ssid: 'DIRECT-zz99:ILCE-7M4' };
  const rows = (root: HTMLElement) =>
    [...root.querySelectorAll('[aria-label="Cameras"] .k')].map((k) => k.textContent);
  function mountWith(recent: RecentCamera[], lastIp: string, model: string) {
    const camera = { ...(cameraState as unknown as CameraState), model, connected: true };
    const store = createStore<AppState>({
      ...initialState(),
      screen: 'main',
      camera,
      settings: { ...settings, recentCameras: recent, lastIp },
    });
    const actions = { connectRecent: vi.fn(), disconnect: vi.fn() } as unknown as Actions;
    const root = document.createElement('div');
    const unmount = mount(root, store, actions);
    return { root, unmount };
  }

  test('a second Wi-Fi Direct camera at the same address still shows while connected to the first', () => {
    const { root, unmount } = mountWith([zv, a7], zv.ip, 'ZV-E10');
    expect(rows(root)).toEqual(['ZV-E10', 'ILCE-7M4', 'Disconnect']);
    unmount();
  });

  test('a Phase 1 entry without a port is the same camera as one with :15740', () => {
    const old: RecentCamera = { ip: '10.0.0.5', model: 'A7 IV', lastUsed: 1, ssid: null };
    const { root, unmount } = mountWith([{ ...old, ip: '192.168.122.1' }, old], '192.168.122.1:15740', 'A7 IV');
    expect(rows(root)).toEqual(['A7 IV', 'A7 IV', 'Disconnect']);
    unmount();
  });
});

describe('Scopes and timer', () => {
  test('the Interval timer row is live and opens its popover', () => {
    const camera = { ...(cameraState as unknown as CameraState), connected: true };
    const store = createStore<AppState>({ ...initialState(), screen: 'main', camera, settings });
    const root = document.createElement('div');
    document.body.append(root);
    const unmount = mount(root, store, createActions(store));
    const rows = [...root.querySelectorAll<HTMLButtonElement>('.leftcol .sect:first-child .row-btn > button')];
    const row = rows.find((b) => b.querySelector('.k')!.textContent === 'Interval timer')!;
    expect(row.title).toBe('');
    expect(row.querySelector('.v')!.textContent).toBe('Off');
    row.click();
    expect(root.querySelector<HTMLElement>('.interval-pop')!.hidden).toBe(false);
    // Flipping an overlay re-renders the switches but keeps the timer row.
    root.querySelector<HTMLButtonElement>('[aria-label="Zebra"]')!.click();
    expect(row.isConnected).toBe(true);
    expect(row.parentElement!.parentElement!.lastElementChild).toBe(row.parentElement);
    unmount();
    expect(root.querySelector('.interval-pop')).toBeNull();
    root.remove();
  });

  test('Waveform and False colour switches flip their overlays', () => {
    const store = createStore<AppState>({ ...initialState(), screen: 'main', settings });
    const root = document.createElement('div');
    document.body.append(root);
    const unmount = mount(root, store, createActions(store));
    expect(store.get().overlays.waveform).toBe(false);
    expect(store.get().overlays.falseColour).toBe(false);
    root.querySelector<HTMLButtonElement>('[aria-label="Waveform"]')!.click();
    root.querySelector<HTMLButtonElement>('[aria-label="False colour"]')!.click();
    expect(store.get().overlays.waveform).toBe(true);
    expect(store.get().overlays.falseColour).toBe(true);
    expect(root.querySelector('[aria-label="Waveform"]')!.getAttribute('aria-checked')).toBe('true');
    unmount();
    root.remove();
  });
});
