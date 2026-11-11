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
  bluetooth: true,
  saveToComputer: true,
  copyFeature: true,
  openAs: 'Remote',
};

// A body of the newer generation, whose remote session can list its card.
const gen4 = { ...(cameraState as unknown as CameraState), generation: '4' };

describe('Save to', () => {
  test('a switch turns copying pictures to this computer off and on', () => {
    const camera = { shots_to_computer: true } as unknown as CameraState;
    const store = createStore({ ...initialState(), screen: 'main' as const, settings, camera });
    const saveSettings = vi.fn((next: AppSettings) => {
      store.set({ settings: next });
      return Promise.resolve();
    });
    const section = saveToSection(store, { ...createActions(store), saveSettings });
    const sw = () => section.el.querySelector<HTMLElement>('[role="switch"]')!;
    const note = () => section.el.querySelector<HTMLElement>('.save-note')!;
    expect(sw().getAttribute('aria-label')).toBe('Copy to this computer');
    expect(sw().getAttribute('aria-checked')).toBe('true');
    expect(note().hidden).toBe(true);

    sw().click();
    expect(saveSettings).toHaveBeenLastCalledWith({ ...settings, saveToComputer: false });
    expect(sw().getAttribute('aria-checked')).toBe('false');
    expect(note().hidden).toBe(false);
    expect(note().textContent).toBe('Pictures and videos stay on the camera\'s card.');
    // Off on purpose: the camera keeping them is not a problem to report.
    store.set({ camera: { ...camera, shots_to_computer: false } });
    expect(section.el.querySelector<HTMLElement>('.save-error')!.hidden).toBe(true);

    sw().click();
    expect(saveSettings).toHaveBeenLastCalledWith({ ...settings, saveToComputer: true });
    expect(note().hidden).toBe(true);
    // On, and the camera still on its card alone: now it is worth saying.
    expect(section.el.querySelector<HTMLElement>('.save-error')!.textContent).toContain('PC+Camera');
    section.destroy();
  });

  test('says when the camera keeps pictures on its card only', () => {
    const camera = { shots_to_computer: false } as unknown as CameraState;
    const store = createStore({ ...initialState(), screen: 'main' as const, settings, camera });
    const section = saveToSection(store, createActions(store));
    const line = () => section.el.querySelector<HTMLElement>('.save-error')!;
    expect(line().hidden).toBe(false);
    expect(line().textContent).toContain('Still Img. Save Dest. to PC+Camera');
    // Once the camera sends them here, the line goes.
    store.set({ camera: { ...camera, shots_to_computer: true } });
    expect(line().hidden).toBe(true);
    section.destroy();
  });

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
    const camera = { ...gen4, model: 'ILCE-7M4', connected: true };
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
    const camera = { ...gen4, model: 'A7 IV', connected: false };
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
    const camera = { ...gen4, model, connected: true };
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
    expect(rows(root)).toEqual(['ZV-E10', 'Browse card', 'ILCE-7M4', 'Add a camera', 'Disconnect']);
    unmount();
  });

  test('a Phase 1 entry without a port is the same camera as one with :15740', () => {
    const old: RecentCamera = { ip: '10.0.0.5', model: 'A7 IV', lastUsed: 1, ssid: null };
    const { root, unmount } = mountWith([{ ...old, ip: '192.168.122.1' }, old], '192.168.122.1:15740', 'A7 IV');
    expect(rows(root)).toEqual(['A7 IV', 'Browse card', 'A7 IV', 'Add a camera', 'Disconnect']);
    unmount();
  });
});

describe('Cameras with several connected', () => {
  const base = gen4;
  const zv = { ...base, model: 'ZV-E10', connected: true };
  const a7 = { ...base, model: 'ILCE-7M4', connected: true };
  const A = '10.0.0.5:15740';
  const B = 'DIRECT-zz99:ILCE-7M4';
  function mountTwo(recent: RecentCamera[] = [], cameras: Record<string, CameraState> = { [A]: zv, [B]: a7 }) {
    const store = createStore<AppState>({
      ...initialState(),
      screen: 'main',
      camera: zv,
      cameras,
      activeId: A,
      settings: { ...settings, recentCameras: recent, lastIp: '10.0.0.5' },
    });
    const actions = {
      connectRecent: vi.fn(),
      disconnect: vi.fn(),
      disconnectAll: vi.fn(),
      switchCamera: vi.fn(),
      shootAll: vi.fn(),
      addCamera: vi.fn(),
    } as unknown as Actions;
    const root = document.createElement('div');
    const unmount = mount(root, store, actions);
    const buttons = () => [...root.querySelectorAll<HTMLButtonElement>('[aria-label="Cameras"] button')];
    const byKey = (k: string) => buttons().find((b) => b.querySelector('.k')?.textContent === k || b.textContent === k)!;
    return { root, store, actions, unmount, buttons, byKey };
  }

  test('one row per connected camera, then Shoot all, Add a camera, Disconnect and Disconnect all', () => {
    const zvRc: RecentCamera = { ip: '10.0.0.5', model: 'ZV-E10', lastUsed: 3, ssid: null };
    const a7Rc: RecentCamera = { ip: '192.168.122.1', model: 'ILCE-7M4', lastUsed: 2, ssid: B };
    const old: RecentCamera = { ip: '10.0.0.9', model: 'A7 IV', lastUsed: 1, ssid: null };
    const { root, unmount, byKey } = mountTwo([zvRc, a7Rc, old]);
    const texts = [...root.querySelectorAll('[aria-label="Cameras"] button')].map(
      (b) => b.querySelector('.k')?.textContent ?? b.textContent,
    );
    // Remembered cameras that are connected are not listed again.
    expect(texts).toEqual([
      'ZV-E10', 'ILCE-7M4', 'Browse card', 'Shoot all', 'A7 IV', 'Add a camera', 'Disconnect', 'Disconnect all',
    ]);
    expect(byKey('ZV-E10').querySelector('.v')!.textContent).toBe('this');
    expect(byKey('ILCE-7M4').querySelector('.v')?.textContent ?? '').not.toBe('this');
    expect(byKey('ILCE-7M4').querySelector('.dot')).not.toBeNull();
    unmount();
  });

  test('clicking a camera switches to it; the buttons call their actions', () => {
    const { actions, unmount, byKey } = mountTwo();
    byKey('ZV-E10').click();
    expect(actions.switchCamera).not.toHaveBeenCalled();
    byKey('ILCE-7M4').click();
    expect(actions.switchCamera).toHaveBeenCalledWith(B);
    byKey('Shoot all').click();
    expect(actions.shootAll).toHaveBeenCalled();
    byKey('Add a camera').click();
    expect(actions.addCamera).toHaveBeenCalled();
    byKey('Disconnect').click();
    expect(actions.disconnect).toHaveBeenCalledWith();
    byKey('Disconnect all').click();
    expect(actions.disconnectAll).toHaveBeenCalled();
    unmount();
  });

  test('with one camera there is no Shoot all or Disconnect all', () => {
    const { root, unmount } = mountTwo([], { [A]: zv });
    const texts = [...root.querySelectorAll('[aria-label="Cameras"] button')].map(
      (b) => b.querySelector('.k')?.textContent ?? b.textContent,
    );
    expect(texts).toEqual(['ZV-E10', 'Browse card', 'Add a camera', 'Disconnect']);
    unmount();
  });

  test('Browse card opens the card panel for the active camera, only while it is connected', () => {
    const { store, unmount, byKey, buttons } = mountTwo();
    expect(store.get().cardOpen).toBe(false);
    byKey('Browse card').click();
    expect(store.get().cardOpen).toBe(true);
    store.set({ camera: { ...zv, connected: false } });
    expect(buttons().some((b) => b.textContent === 'Browse card')).toBe(false);
    unmount();
  });
});

describe('Scopes and timer', () => {
  test('the Interval timer row is live and opens its popover', () => {
    const camera = { ...gen4, connected: true };
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

describe('Webcam switch', () => {
  test('sits under the cameras, follows the feed and asks for the other state', () => {
    const root = document.createElement('div');
    const store = createStore<AppState>({ ...initialState(), screen: 'main' });
    const setWebcamMode = vi.fn(() => Promise.resolve());
    const unmount = mount(root, store, { ...createActions(store), setWebcamMode });
    const sw = () => root.querySelector<HTMLElement>('.group[aria-label="Webcam"] [role="switch"]')!;
    expect(sw().getAttribute('aria-checked')).toBe('false');
    sw().click();
    expect(setWebcamMode).toHaveBeenCalledWith(true);
    store.set({ feed: { enabled: true, url: 'http://127.0.0.1:8717/', stream_url: null, quality: 'Full', width: null, height: null, viewers: 0 } });
    expect(sw().getAttribute('aria-checked')).toBe('true');
    sw().click();
    expect(setWebcamMode).toHaveBeenLastCalledWith(false);
    // The destination panel is still there, after the tools.
    expect([...root.querySelectorAll('.panel')].map((p) => p.querySelector('.h')!.textContent)).toEqual([
      'Scopes and timer',
      'Save to',
    ]);
    unmount();
  });
});

describe('Bodies whose remote session cannot list the card', () => {
  // Sony's reference gives the ZV-E10 no storage commands: its files are
  // browsed in Send to Smartphone, from the connect screen.
  test('have no Browse card row', () => {
    const root = document.createElement('div');
    const camera = { ...(cameraState as unknown as CameraState), connected: true };
    expect(camera.generation).toBe('3');
    const store = createStore<AppState>({ ...initialState(), screen: 'main', camera, cameras: { a: camera }, activeId: 'a' });
    const unmount = mount(root, store, createActions(store));
    const rows = [...root.querySelectorAll('.group[aria-label="Cameras"] .k')].map((k) => k.textContent);
    expect(rows).not.toContain('Browse card');
    expect(rows).toContain('Disconnect');
    unmount();
  });
});

describe('Copying put away', () => {
  test('the Save to panel shows only once copying after each shot is asked for', () => {
    const root = document.createElement('div');
    const store = createStore<AppState>({ ...initialState(), screen: 'main', settings: { ...settings, copyFeature: false } });
    const unmount = mount(root, store, createActions(store));
    const saveTo = () => root.querySelectorAll<HTMLElement>('.panel')[1];
    expect(saveTo().querySelector('.h')!.textContent).toBe('Save to');
    expect(saveTo().hidden).toBe(true);
    // The tools beside it stay.
    expect(root.querySelectorAll<HTMLElement>('.panel')[0].hidden).toBe(false);
    store.set({ settings: { ...settings, copyFeature: true } });
    expect(saveTo().hidden).toBe(false);
    unmount();
  });

  test('a camera keeping pictures on its card is no fault while copying is put away', () => {
    const camera = { shots_to_computer: false } as unknown as CameraState;
    const store = createStore({ ...initialState(), screen: 'main' as const, settings: { ...settings, copyFeature: false }, camera });
    const section = saveToSection(store, createActions(store));
    expect(section.el.querySelector<HTMLElement>('.save-error')!.hidden).toBe(true);
    expect(section.el.querySelector<HTMLElement>('.save-note')!.hidden).toBe(true);
    section.destroy();
  });
});
