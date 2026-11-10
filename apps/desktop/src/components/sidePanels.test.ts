import { describe, expect, test, vi } from 'vitest';
import cameraState from '../fixtures/camera-state.json';
import type { Actions } from '../actions';
import type { AppSettings, CameraState, RecentCamera } from '../ipc';
import { createActions } from '../actions';
import { createStore, initialState } from '../state';
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
});
