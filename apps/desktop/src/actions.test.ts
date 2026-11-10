import { beforeEach, describe, expect, test, vi } from 'vitest';
import cameraState from './fixtures/camera-state.json';
import { ASK_FIND_MS, createActions, FIND_MS } from './actions';
import { start } from './interval';
import * as ipc from './ipc';
import type { AppSettings, CameraState, DiscoveredCamera, Preset, RecentCamera, WifiJoin } from './ipc';
import { createStore, defaultSettings, initialState, type AppState } from './state';

vi.mock('./ipc', () => ({
  connect: vi.fn(),
  getState: vi.fn(),
  getSettings: vi.fn(),
  saveSettings: vi.fn(),
  joinNetwork: vi.fn(),
  waitForCamera: vi.fn(),
  disconnect: vi.fn(),
  applyValues: vi.fn(),
}));

const camera = cameraState as unknown as CameraState;
const REFUSED = 'Cannot connect to 10.0.0.9:15740: I/O error: Connection refused (os error 61)';
const SSID = 'DIRECT-abc1:ZV-E10';
const found: DiscoveredCamera = {
  model: 'ZV-E10',
  friendly_name: 'ZV-E10',
  ip: '192.168.122.1',
  ptp_port: 15740,
  ptp_version: null,
  pairing_required: null,
  ssh_supported: null,
  source: 'Gateway',
};

function storeWith(wifiJoin: WifiJoin, extra: Partial<AppSettings> = {}, state: Partial<AppState> = {}) {
  const settings = { ...defaultSettings('/tmp'), wifiJoin, ...extra };
  return createStore({ ...initialState(), settings, ...state });
}

beforeEach(() => {
  vi.mocked(ipc.connect).mockReset().mockResolvedValue(camera);
  vi.mocked(ipc.getState).mockReset().mockResolvedValue(null);
  vi.mocked(ipc.getSettings).mockReset().mockRejectedValue('no settings');
  vi.mocked(ipc.saveSettings).mockReset().mockResolvedValue();
  vi.mocked(ipc.joinNetwork).mockReset().mockResolvedValue();
  vi.mocked(ipc.waitForCamera).mockReset().mockResolvedValue(found);
  vi.mocked(ipc.disconnect).mockReset().mockResolvedValue();
});

describe('connect from the main screen', () => {
  beforeEach(() => {
    vi.mocked(ipc.connect).mockRejectedValue(REFUSED);
  });

  test('a failed switch that lost the old camera returns to the connect screen', async () => {
    const store = createStore({ ...initialState(), screen: 'main' as const, camera });
    expect(await createActions(store).connect('10.0.0.9')).toBe(false);
    expect(store.get().screen).toBe('connect');
    expect(store.get().camera).toBeNull();
    expect(store.get().connecting).toBeNull();
    expect(store.get().error).toContain('Could not reach the camera at 10.0.0.9.');
  });

  test('a failed connect keeps a camera that is still connected', async () => {
    vi.mocked(ipc.getState).mockResolvedValue(camera);
    const store = createStore({ ...initialState(), screen: 'main' as const, camera });
    expect(await createActions(store).connect('10.0.0.9')).toBe(false);
    expect(store.get().screen).toBe('main');
    expect(store.get().camera).toBe(camera);
  });
});

describe('connect over Wi-Fi', () => {
  test('connectWithWifi joins, finds and connects, remembering the network', async () => {
    const store = storeWith('Automatic');
    const actions = createActions(store);
    const phases: string[] = [];
    store.subscribe((s) => {
      if (s.connecting) phases.push(s.connecting.phase);
    });
    expect(await actions.connectWithWifi({ ssid: SSID, password: '12345678' })).toBe(true);
    expect(phases).toEqual(['joining', 'finding', 'connecting']);
    expect(ipc.joinNetwork).toHaveBeenCalledWith(SSID, '12345678');
    expect(ipc.waitForCamera).toHaveBeenCalledWith(FIND_MS);
    expect(ipc.connect).toHaveBeenCalledWith('192.168.122.1:15740', SSID, null);
    expect(store.get().connecting).toBeNull();
    expect(store.get().screen).toBe('main');
  });

  test('connectWithWifi explains when no camera answers and frees the screen', async () => {
    vi.mocked(ipc.waitForCamera).mockRejectedValue('No camera answered on this network');
    const store = storeWith('Automatic');
    expect(await createActions(store).connectWithWifi({ ssid: SSID, password: 'x' })).toBe(false);
    expect(store.get().connecting).toBeNull();
    expect(store.get().error).toMatch(/no camera answered.*PC Remote Function/);
    expect(ipc.connect).not.toHaveBeenCalled();
  });

  test('a failed join says what happened and does not look for the camera', async () => {
    vi.mocked(ipc.joinNetwork).mockRejectedValue('This computer has no Wi-Fi');
    const store = storeWith('Automatic');
    expect(await createActions(store).connectWithWifi({ ssid: SSID, password: 'x' })).toBe(false);
    expect(store.get().connecting).toBeNull();
    expect(store.get().error).toBe(
      'This computer has no Wi-Fi. Put the camera and this computer on the same router and use Nearby.',
    );
    expect(ipc.waitForCamera).not.toHaveBeenCalled();
  });

  test('Ask mode skips joining and waits longer', async () => {
    const store = storeWith('Ask');
    const texts: string[] = [];
    store.subscribe((s) => {
      if (s.connecting) texts.push(s.connecting.text);
    });
    expect(await createActions(store).connectWithWifi({ ssid: SSID, password: 'x' })).toBe(true);
    expect(ipc.joinNetwork).not.toHaveBeenCalled();
    expect(ipc.waitForCamera).toHaveBeenCalledWith(ASK_FIND_MS);
    expect(texts[0]).toContain('from the Wi-Fi menu');
  });

  test('Ask mode explains a silent network without claiming Rimix joined it', async () => {
    vi.mocked(ipc.waitForCamera).mockRejectedValue('No camera answered on this network');
    const store = storeWith('Ask');
    expect(await createActions(store).connectWithWifi({ ssid: SSID, password: 'x' })).toBe(false);
    expect(store.get().connecting).toBeNull();
    expect(store.get().error).toMatch(/^No camera answered on DIRECT-abc1:ZV-E10\./);
    expect(store.get().error).not.toMatch(/^Joined/);
  });

  test('connectRecent uses the network when known and the address otherwise', async () => {
    const store = storeWith('Automatic');
    const actions = createActions(store);
    const withSsid: RecentCamera = { ip: '192.168.122.1', model: 'ZV-E10', lastUsed: 1, ssid: SSID };
    expect(await actions.connectRecent(withSsid)).toBe(true);
    expect(ipc.joinNetwork).toHaveBeenCalledWith(SSID, null);
    expect(ipc.connect).toHaveBeenLastCalledWith('192.168.122.1:15740', SSID, null);

    vi.mocked(ipc.joinNetwork).mockClear();
    expect(await actions.connectRecent({ ...withSsid, ip: '10.0.0.5', ssid: null })).toBe(true);
    expect(ipc.joinNetwork).not.toHaveBeenCalled();
    expect(ipc.connect).toHaveBeenLastCalledWith('10.0.0.5', null, null);
  });

  test('a connect in flight is not started twice', async () => {
    const store = storeWith('Automatic', {}, { connecting: { phase: 'finding', text: 'Looking' } });
    const actions = createActions(store);
    expect(await actions.connect('10.0.0.5')).toBe(false);
    expect(await actions.connectRecent({ ip: '10.0.0.5', model: 'A7 IV', lastUsed: 1, ssid: null })).toBe(false);
    expect(await actions.connectWithWifi({ ssid: SSID, password: 'x' })).toBe(false);
    expect(ipc.connect).not.toHaveBeenCalled();
    expect(ipc.joinNetwork).not.toHaveBeenCalled();
  });

  test('a connect to another camera forgets the active preset', async () => {
    const store = storeWith('Automatic', {}, { screen: 'main', camera, activePreset: 2 });
    expect(await createActions(store).connect('10.0.0.9')).toBe(true);
    expect(store.get().activePreset).toBeNull();
  });

  test('a successful connect clears an error set while it ran', async () => {
    let finish: (c: CameraState) => void = () => {};
    vi.mocked(ipc.connect).mockReturnValue(new Promise((r) => (finish = r)));
    const store = storeWith('Automatic');
    const done = createActions(store).connect('192.168.122.1');
    store.set({ error: 'stale' });
    finish(camera);
    expect(await done).toBe(true);
    expect(store.get().error).toBeNull();
  });

  test('forgetCamera drops the entry and clears lastIp', async () => {
    const a: RecentCamera = { ip: '192.168.122.1', model: 'ZV-E10', lastUsed: 2, ssid: SSID };
    const b: RecentCamera = { ip: '10.0.0.5', model: 'A7 IV', lastUsed: 1, ssid: null };
    const store = storeWith('Automatic', { recentCameras: [a, b], lastIp: a.ip });
    await createActions(store).forgetCamera(a);
    const saved = vi.mocked(ipc.saveSettings).mock.calls[0][0];
    expect(saved.recentCameras).toEqual([b]);
    expect(saved.lastIp).toBeNull();
    expect(store.get().settings?.recentCameras).toEqual([b]);

    await createActions(store).forgetCamera(b);
    expect(vi.mocked(ipc.saveSettings).mock.calls[1][0]).toMatchObject({ recentCameras: [], lastIp: null });
  });

  test('forgetCamera removes only the camera on that network', async () => {
    const a: RecentCamera = { ip: '192.168.122.1', model: 'ZV-E10', lastUsed: 3, ssid: SSID };
    const b: RecentCamera = { ip: '192.168.122.1', model: 'ILCE-7M4', lastUsed: 2, ssid: 'DIRECT-zz99:ILCE-7M4' };
    const c: RecentCamera = { ip: '192.168.122.1', model: 'A7 IV', lastUsed: 1, ssid: null };
    const store = storeWith('Automatic', { recentCameras: [a, b, c], lastIp: a.ip });
    await createActions(store).forgetCamera(a);
    expect(store.get().settings).toMatchObject({ recentCameras: [b, c], lastIp: a.ip });
    await createActions(store).forgetCamera(c);
    expect(store.get().settings).toMatchObject({ recentCameras: [b], lastIp: a.ip });
  });

  test('forgetCamera keeps lastIp pointing at another camera', async () => {
    const a: RecentCamera = { ip: '192.168.122.1', model: 'ZV-E10', lastUsed: 2, ssid: SSID };
    const b: RecentCamera = { ip: '10.0.0.5', model: 'A7 IV', lastUsed: 1, ssid: null };
    const store = storeWith('Automatic', { recentCameras: [a, b], lastIp: a.ip });
    await createActions(store).forgetCamera(b);
    expect(store.get().settings).toMatchObject({ recentCameras: [a], lastIp: a.ip });
  });
});

describe('Access Authentication', () => {
  const SHA = 'SHA256:q1w2e3r4t5y6u7i8o9p0asdfghjklzxcvbnm1234567';
  const MD5 = 'aa:bb:cc:dd:ee:ff:00:11:22:33:44:55:66:77:88:99';
  const login = { user: 'cam', password: '12345678', fingerprint: null };

  test('an unconfirmed fingerprint waits for the user instead of failing', async () => {
    vi.mocked(ipc.connect).mockRejectedValueOnce(`HOSTKEY ${SHA} ${MD5}`);
    const store = storeWith('Automatic');
    expect(await createActions(store).connect('192.168.122.1', null, login)).toBe(false);
    expect(ipc.connect).toHaveBeenCalledWith('192.168.122.1', null, login);
    const pending = store.get().pendingFingerprint;
    expect(pending).toMatchObject({ sha256: SHA, md5: MD5 });
    expect(store.get().error).toBeNull();
    expect(store.get().connecting).toBeNull();
    expect(store.get().screen).toBe('connect');
  });

  test('confirming re-runs the same connect with the fingerprint', async () => {
    vi.mocked(ipc.connect).mockRejectedValueOnce(`HOSTKEY ${SHA} ${MD5}`);
    const store = storeWith('Automatic');
    const actions = createActions(store);
    await actions.connectWithWifi({ ssid: SSID, password: 'x' }, login);
    expect(ipc.connect).toHaveBeenLastCalledWith('192.168.122.1:15740', SSID, login);
    vi.mocked(ipc.joinNetwork).mockClear();
    expect(await store.get().pendingFingerprint!.retry()).toBe(true);
    expect(ipc.connect).toHaveBeenLastCalledWith('192.168.122.1:15740', SSID, { ...login, fingerprint: SHA });
    // The network is already joined: only the connect runs again.
    expect(ipc.joinNetwork).not.toHaveBeenCalled();
    expect(store.get().pendingFingerprint).toBeNull();
    expect(store.get().screen).toBe('main');
  });

  test('a new connect attempt drops a fingerprint waiting for confirmation', async () => {
    const store = storeWith('Automatic', {}, {
      pendingFingerprint: { sha256: SHA, md5: MD5, retry: () => Promise.resolve(true) },
    });
    await createActions(store).connect('10.0.0.5');
    expect(store.get().pendingFingerprint).toBeNull();
  });

  test('a remembered fingerprint is used when the login has none', async () => {
    const rc: RecentCamera = {
      ip: '192.168.122.1:15740', model: 'ILCE-7M4', lastUsed: 1, ssid: SSID, sshUser: 'cam', sshFingerprint: SHA,
    };
    const store = storeWith('Automatic', { recentCameras: [rc] });
    await createActions(store).connectWithWifi({ ssid: SSID, password: '' }, login);
    expect(ipc.connect).toHaveBeenLastCalledWith('192.168.122.1:15740', SSID, { ...login, fingerprint: SHA });
  });

  test('a remembered Access Authentication camera asks for its password instead of connecting', async () => {
    const rc: RecentCamera = {
      ip: '192.168.122.1', model: 'ILCE-7M4', lastUsed: 1, ssid: SSID, sshUser: 'cam', sshFingerprint: SHA,
    };
    const store = storeWith('Automatic', { recentCameras: [rc] }, { screen: 'main', camera });
    expect(await createActions(store).connectRecent(rc)).toBe(false);
    expect(ipc.connect).not.toHaveBeenCalled();
    expect(ipc.joinNetwork).not.toHaveBeenCalled();
    expect(store.get().screen).toBe('connect');
    expect(store.get().sshPrefill).toEqual({ user: 'cam', ssid: SSID, ip: '192.168.122.1' });
  });

  test('switching to a remembered Access Authentication camera drops the current one first', async () => {
    const rc: RecentCamera = {
      ip: '192.168.122.1', model: 'ILCE-7M4', lastUsed: 1, ssid: SSID, sshUser: 'cam', sshFingerprint: SHA,
    };
    const store = storeWith('Automatic', { recentCameras: [rc] }, {
      screen: 'main', camera, reconnecting: 3, captures: [{ path: '/p/a.JPG', filename: 'a.JPG', bytes: 1 }],
    });
    expect(await createActions(store).connectRecent(rc)).toBe(false);
    expect(ipc.disconnect).toHaveBeenCalledTimes(1);
    expect(store.get().camera).toBeNull();
    expect(store.get().reconnecting).toBeNull();
    expect(store.get().captures).toEqual([]);
    expect(store.get().screen).toBe('connect');
    expect(store.get().sshPrefill).toEqual({ user: 'cam', ssid: SSID, ip: '192.168.122.1' });
  });
});

describe('presets', () => {
  const preset: Preset = {
    id: 'a',
    name: 'Talking head',
    model: 'ZV-E10',
    slot: 2,
    values: [
      { code: 0xd21e, raw: { U32: 1600 } },
      { code: 0x5007, raw: { U16: 9999 } },
    ],
  };
  const other: Preset = { ...preset, id: 'b', slot: 3, name: 'Night' };

  test('applyPreset is busy while it runs and toasts what failed', async () => {
    vi.mocked(ipc.applyValues).mockResolvedValue([
      { code: 0xd21e, error: null },
      { code: 0x5007, error: 'Value not allowed' },
    ]);
    const store = storeWith('Automatic', {}, { camera, activePreset: 1 });
    const toast = vi.fn();
    const busy: (number | null)[] = [];
    store.subscribe((s) => busy.push(s.presetBusy));
    await createActions(store, toast).applyPreset(preset);
    expect(ipc.applyValues).toHaveBeenCalledWith(preset.values);
    expect(busy[0]).toBe(2);
    expect(store.get().presetBusy).toBeNull();
    expect(toast).toHaveBeenCalledWith('Could not set Aperture');
    // Not a full success, so the previous active preset stays.
    expect(store.get().activePreset).toBe(1);
  });

  test('applyPreset marks the slot active on full success', async () => {
    vi.mocked(ipc.applyValues).mockResolvedValue([
      { code: 0xd21e, error: null },
      { code: 0x5007, error: null },
    ]);
    const store = storeWith('Automatic', {}, { camera });
    const toast = vi.fn();
    await createActions(store, toast).applyPreset(preset);
    expect(toast).not.toHaveBeenCalled();
    expect(store.get().activePreset).toBe(2);
    expect(store.get().presetBusy).toBeNull();
  });

  test('applyPreset clears busy and toasts when the command fails', async () => {
    vi.mocked(ipc.applyValues).mockRejectedValue('Not connected to a camera');
    const store = storeWith('Automatic', {}, { camera });
    const toast = vi.fn();
    await createActions(store, toast).applyPreset(preset);
    expect(toast).toHaveBeenCalledWith('Not connected to a camera');
    expect(store.get().presetBusy).toBeNull();
    expect(store.get().activePreset).toBeNull();
  });

  test('savePreset adds or replaces the slot and saves the settings', async () => {
    const store = storeWith('Automatic', { presets: [other] });
    const actions = createActions(store);
    await actions.savePreset(preset);
    expect(vi.mocked(ipc.saveSettings).mock.calls[0][0].presets).toEqual([other, preset]);
    const renamed = { ...preset, name: 'Vlog' };
    await actions.savePreset(renamed);
    expect(vi.mocked(ipc.saveSettings).mock.calls[1][0].presets).toEqual([other, renamed]);
    expect(store.get().settings?.presets).toEqual([other, renamed]);
  });

  test('deletePreset drops it by id', async () => {
    const store = storeWith('Automatic', { presets: [preset, other] });
    const actions = createActions(store);
    store.set({ activePreset: 3 });
    await actions.deletePreset('a');
    expect(vi.mocked(ipc.saveSettings).mock.calls[0][0].presets).toEqual([other]);
    // Another slot was active, so it stays.
    expect(store.get().activePreset).toBe(3);
    // Deleting the active slot's preset clears it.
    await actions.deletePreset('b');
    expect(store.get().activePreset).toBeNull();
  });

  test('disconnect forgets the active preset', async () => {
    vi.mocked(ipc.disconnect).mockResolvedValue();
    const store = storeWith('Automatic', {}, { camera, activePreset: 2 });
    await createActions(store).disconnect();
    expect(store.get().activePreset).toBeNull();
  });
});

describe('interval timer', () => {
  test('startInterval starts a run one period out and stopInterval clears it', () => {
    vi.useFakeTimers({ now: 10_000 });
    try {
      const store = storeWith('Automatic', {}, { camera });
      const actions = createActions(store);
      actions.startInterval({ everyS: 1, count: 3 });
      expect(store.get().interval).toEqual({
        plan: { everyS: 2, count: 3 }, taken: 0, missed: 0, nextAt: 12_000, running: true,
      });
      actions.stopInterval();
      expect(store.get().interval).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  test('disconnect stops a running interval', async () => {
    const store = storeWith('Automatic', {}, { camera, interval: start({ everyS: 5, count: null }, 0) });
    await createActions(store).disconnect();
    expect(store.get().interval).toBeNull();
  });
});
