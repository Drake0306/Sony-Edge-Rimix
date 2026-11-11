import { beforeEach, describe, expect, test, vi } from 'vitest';
import cameraState from './fixtures/camera-state.json';
import cardObject from './fixtures/card-object.json';
import { ASK_FIND_MS, createActions, FIND_MS, parseHostKeyChanged, rememberedFingerprint } from './actions';
import { mount as mountTopbar } from './components/topbar';
import { start } from './interval';
import * as ipc from './ipc';
import type { AppSettings, CameraState, CardObject, DiscoveredCamera, Preset, RecentCamera, WifiJoin } from './ipc';
import { applyEvent, createStore, defaultSettings, emptyCard, initialState, type AppState } from './state';

vi.mock('./ipc', () => ({
  connect: vi.fn(),
  getState: vi.fn(),
  getSettings: vi.fn(),
  saveSettings: vi.fn(),
  joinNetwork: vi.fn(),
  waitForCamera: vi.fn(),
  disconnect: vi.fn(),
  disconnectAll: vi.fn(),
  applyValues: vi.fn(),
  listCameras: vi.fn(),
  setActiveCamera: vi.fn(),
  shoot: vi.fn(),
  shootAll: vi.fn(),
  cardList: vi.fn(),
  cardDownload: vi.fn(),
  cardMode: vi.fn(),
  bleHandoff: vi.fn(),
  cancelConnect: vi.fn(() => Promise.resolve()),
  focusAt: vi.fn(() => Promise.resolve()),
  webcamSet: vi.fn(),
  filesOpen: vi.fn(),
  filesList: vi.fn(),
  filesImport: vi.fn(),
  filesClose: vi.fn(() => Promise.resolve()),
}));

const camera = cameraState as unknown as CameraState;
const a7 = { ...camera, model: 'ILCE-7M4' };
const ID = '192.168.122.1:15740';
const A = '10.0.0.5:15740';
const B = 'DIRECT-zz99:ILCE-7M4';
const summary = (id: string, active = false) => ({ id, label: 'ZV-E10', connected: true, secured: false, active });
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
  vi.mocked(ipc.connect).mockReset().mockResolvedValue({ id: ID, state: camera });
  vi.mocked(ipc.listCameras).mockReset().mockResolvedValue([]);
  vi.mocked(ipc.setActiveCamera).mockReset().mockResolvedValue();
  vi.mocked(ipc.shoot).mockReset().mockResolvedValue();
  vi.mocked(ipc.shootAll).mockReset().mockResolvedValue([]);
  vi.mocked(ipc.disconnectAll).mockReset().mockResolvedValue();
  vi.mocked(ipc.getState).mockReset().mockResolvedValue(null);
  vi.mocked(ipc.getSettings).mockReset().mockRejectedValue('no settings');
  vi.mocked(ipc.saveSettings).mockReset().mockResolvedValue();
  vi.mocked(ipc.joinNetwork).mockReset().mockResolvedValue();
  vi.mocked(ipc.waitForCamera).mockReset().mockResolvedValue(found);
  vi.mocked(ipc.disconnect).mockReset().mockResolvedValue();
  vi.mocked(ipc.bleHandoff).mockReset().mockResolvedValue({ ssid: SSID, password: '12345678' });
});

// Roy, 2026-11-11: choose before connecting whether the camera is for
// remote shooting, a webcam, or looking through its files.
describe('what the camera was connected for', () => {
  const feed = (enabled: boolean) => ({ enabled, url: 'http://127.0.0.1:8717/', stream_url: null, quality: 'Full' as const, width: null, height: null, viewers: 0 });
  const connectAs = async (openAs: AppSettings['openAs'], state: Partial<AppState> = {}) => {
    const settings = { ...defaultSettings('/tmp'), openAs };
    vi.mocked(ipc.getSettings).mockResolvedValue(settings);
    vi.mocked(ipc.webcamSet).mockReset().mockImplementation((on: boolean) => Promise.resolve(feed(on)));
    const store = createStore({ ...initialState(), settings, ...state });
    expect(await createActions(store).connect('10.0.0.5')).toBe(true);
    await Promise.resolve();
    return store;
  };

  test('remote shooting opens on the live view and the controls', async () => {
    const store = await connectAs('Remote');
    expect(store.get().screen).toBe('main');
    expect(store.get().cardOpen).toBe(false);
    expect(ipc.webcamSet).not.toHaveBeenCalled();
  });

  test('a webcam opens with its picture going to other apps', async () => {
    const store = await connectAs('Webcam');
    expect(ipc.webcamSet).toHaveBeenCalledWith(true);
    expect(store.get().feed?.enabled).toBe(true);
    expect(store.get().cardOpen).toBe(false);
  });

  test('anything but a webcam takes the picture back from other apps', async () => {
    const store = await connectAs('Remote', { feed: feed(true) });
    expect(ipc.webcamSet).toHaveBeenCalledWith(false);
    expect(store.get().feed?.enabled).toBe(false);
  });
});

// Browsing files is the camera's other mode (Send to Smartphone): no remote
// session is made, and the files screen opens instead of the live view.
describe('browsing a camera\'s files', () => {
  const info = { model: 'ZV-E10', root: 'PushRoot', address: '192.168.122.1:64321' };
  const file = (id: string, title: string, folder = false): ipc.FileEntry => ({
    id, title, folder, date: null, thumb: folder ? null : `http://c/TN_${title}`, preview: folder ? null : `http://c/LRG_${title}`,
    file: folder ? null : `http://c/ORG_${title}`, bytes: folder ? null : 100, resolution: null, original: !folder,
  });
  const filesStore = () => storeWith('Automatic', { openAs: 'Files' });
  beforeEach(() => {
    vi.mocked(ipc.connect).mockClear();
    vi.mocked(ipc.waitForCamera).mockClear();
    vi.mocked(ipc.joinNetwork).mockReset().mockResolvedValue(undefined as never);
    vi.mocked(ipc.filesOpen).mockReset().mockResolvedValue(info);
    vi.mocked(ipc.filesList).mockReset().mockImplementation((folder: string | null = null) =>
      Promise.resolve(folder === null ? [file('d1', '2026-11-11', true)] : [file('a', 'DSC00412.JPG'), file('b', 'DSC00413.JPG')]),
    );
    vi.mocked(ipc.filesImport).mockReset().mockResolvedValue('/shots/DSC00412.JPG');
  });

  test('joining the camera\'s Wi-Fi opens its files, with no remote session', async () => {
    const store = filesStore();
    expect(await createActions(store).connectWithWifi({ ssid: SSID, password: '12345678' })).toBe(true);
    expect(ipc.joinNetwork).toHaveBeenCalledWith(SSID, '12345678');
    expect(ipc.filesOpen).toHaveBeenCalledWith(null);
    expect(ipc.waitForCamera).not.toHaveBeenCalled();
    expect(ipc.connect).not.toHaveBeenCalled();
    const s = store.get();
    expect(s.screen).toBe('files');
    expect(s.connecting).toBeNull();
    expect(s.files!.info).toEqual(info);
    // The single folder at the top is opened at once.
    expect(s.files!.path.map((p) => p.title)).toEqual(['ZV-E10', '2026-11-11']);
    expect(s.files!.items.map((f) => f.title)).toEqual(['DSC00412.JPG', 'DSC00413.JPG']);
  });

  test('by address the same, at that address', async () => {
    const store = filesStore();
    expect(await createActions(store).connect('192.168.122.1')).toBe(true);
    expect(ipc.filesOpen).toHaveBeenCalledWith('192.168.122.1');
    expect(ipc.connect).not.toHaveBeenCalled();
  });

  test('a camera not in Send to Smartphone is said, with what to do on it', async () => {
    vi.mocked(ipc.filesOpen).mockRejectedValue('NO_SEND_TO');
    const store = filesStore();
    expect(await createActions(store).connect('192.168.122.1')).toBe(false);
    expect(store.get().screen).toBe('connect');
    expect(store.get().error).toContain('Send to Smartphone');
    expect(store.get().connecting).toBeNull();
  });

  test('a file is imported, shown as it arrives, then as imported; a failure stays on its file', async () => {
    const store = filesStore();
    const actions = createActions(store);
    await actions.connect('192.168.122.1');
    const [a, b] = store.get().files!.items;
    let done!: (path: string) => void;
    vi.mocked(ipc.filesImport).mockImplementationOnce(() => new Promise((ok) => (done = ok)));
    const importing = actions.filesImport(a);
    expect(store.get().files!.progress).toEqual({ a: { received: 0, total: 100 } });
    done('/shots/DSC00412.JPG');
    await importing;
    expect(ipc.filesImport).toHaveBeenCalledWith('a', 'http://c/ORG_DSC00412.JPG', 'DSC00412.JPG');
    expect(store.get().files!.progress).toEqual({});
    expect(store.get().files!.saved).toEqual({ a: '/shots/DSC00412.JPG' });

    vi.mocked(ipc.filesImport).mockRejectedValueOnce('the camera does not send DSC00413.JPG over Wi-Fi (answered 406)');
    await actions.filesImport(b);
    expect(store.get().files!.failed.b).toContain('406');
    expect(store.get().files!.saved).toEqual({ a: '/shots/DSC00412.JPG' });
  });

  test('Import all takes what is not imported yet; Done tells the camera and goes back', async () => {
    const store = filesStore();
    const actions = createActions(store);
    await actions.connect('192.168.122.1');
    await actions.filesImport(store.get().files!.items[0]);
    vi.mocked(ipc.filesImport).mockClear();
    await actions.filesImportAll();
    expect(vi.mocked(ipc.filesImport).mock.calls.map((c) => c[0])).toEqual(['b']);
    await actions.filesDone();
    expect(ipc.filesClose).toHaveBeenCalled();
    expect(store.get().screen).toBe('connect');
    expect(store.get().files).toBeNull();
  });
});

describe('connect from the main screen', () => {
  beforeEach(() => {
    vi.mocked(ipc.connect).mockRejectedValue(REFUSED);
  });

  test('a failed reconnect that lost the only camera returns to the connect screen', async () => {
    const store = createStore({ ...initialState(), screen: 'main' as const, camera, cameras: { [A]: camera }, activeId: A });
    expect(await createActions(store).connect('10.0.0.9')).toBe(false);
    expect(store.get().screen).toBe('connect');
    expect(store.get().camera).toBeNull();
    expect(store.get().cameras).toEqual({});
    expect(store.get().activeId).toBeNull();
    expect(store.get().connecting).toBeNull();
    expect(store.get().error).toContain('Could not reach the camera at 10.0.0.9.');
  });

  test('a failed connect keeps a camera that is still connected', async () => {
    vi.mocked(ipc.listCameras).mockResolvedValue([summary(A, true)]);
    const store = createStore({ ...initialState(), screen: 'main' as const, camera, cameras: { [A]: camera }, activeId: A });
    expect(await createActions(store).connect('10.0.0.9')).toBe(false);
    expect(store.get().screen).toBe('main');
    expect(store.get().camera).toBe(camera);
    expect(store.get().cameras).toEqual({ [A]: camera });
  });

  test('a failed reconnect of the active camera falls back to the one still connected', async () => {
    vi.mocked(ipc.listCameras).mockResolvedValue([summary(A, true)]);
    const store = createStore({
      ...initialState(), screen: 'main' as const, camera: a7, cameras: { [A]: camera, [B]: a7 }, activeId: B,
    });
    expect(await createActions(store).connect('10.0.0.9')).toBe(false);
    expect(store.get().cameras).toEqual({ [A]: camera });
    expect(store.get().activeId).toBe(A);
    expect(store.get().camera).toBe(camera);
    expect(store.get().screen).toBe('main');
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

  test('connectWithWifi names the Local Network setting when the system blocks Rimix', async () => {
    vi.mocked(ipc.waitForCamera).mockRejectedValue('This computer is blocking Rimix from the local network');
    const store = storeWith('Automatic');
    expect(await createActions(store).connectWithWifi({ ssid: SSID, password: 'x' })).toBe(false);
    expect(store.get().connecting).toBeNull();
    expect(store.get().error).toMatch(/Privacy & Security, Local Network/);
    expect(store.get().error).not.toMatch(/PC Remote Function/);
    expect(ipc.connect).not.toHaveBeenCalled();
  });

  test('a cancelled connect frees the screen and is not an error', async () => {
    vi.mocked(ipc.joinNetwork).mockRejectedValue('cancelled');
    const store = storeWith('Automatic');
    const actions = createActions(store);
    expect(await actions.connectWithWifi({ ssid: SSID, password: 'x' })).toBe(false);
    expect(store.get().connecting).toBeNull();
    expect(store.get().error).toBeNull();
    // Cancelling asks the app to stop whichever step is running.
    await actions.cancelConnect();
    expect(ipc.cancelConnect).toHaveBeenCalled();
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

  test('a connect adds a camera beside the ones connected and makes it active', async () => {
    vi.mocked(ipc.connect).mockResolvedValue({ id: B, state: a7 });
    const store = storeWith('Automatic', {}, { screen: 'main', camera, cameras: { [A]: camera }, activeId: A });
    expect(await createActions(store).connect('10.0.0.9')).toBe(true);
    expect(ipc.disconnect).not.toHaveBeenCalled();
    expect(store.get().cameras).toEqual({ [A]: camera, [B]: a7 });
    expect(store.get().activeId).toBe(B);
    expect(store.get().camera).toBe(a7);
    expect(store.get().screen).toBe('main');
    expect(store.get().keepCameras).toBe(false);
  });

  test('the same camera connected under another id replaces its entry', async () => {
    vi.mocked(ipc.connect).mockResolvedValue({ id: A, state: camera });
    vi.mocked(ipc.listCameras).mockResolvedValue([summary(B), summary(A, true)]);
    const store = storeWith('Automatic', {}, {
      screen: 'main', camera, cameras: { [SSID]: camera, [B]: a7 }, activeId: SSID,
    });
    expect(await createActions(store).connect('10.0.0.5')).toBe(true);
    expect(Object.keys(store.get().cameras)).toEqual([B, A]);
    expect(store.get().activeId).toBe(A);
  });

  test('a connect to another camera forgets the active preset', async () => {
    const store = storeWith('Automatic', {}, { screen: 'main', camera, activePreset: 2 });
    expect(await createActions(store).connect('10.0.0.9')).toBe(true);
    expect(store.get().activePreset).toBeNull();
  });

  test('a successful connect clears an error set while it ran', async () => {
    let finish: (c: ipc.ConnectResult) => void = () => {};
    vi.mocked(ipc.connect).mockReturnValue(new Promise((r) => (finish = r)));
    const store = storeWith('Automatic');
    const done = createActions(store).connect('192.168.122.1');
    store.set({ error: 'stale' });
    finish({ id: ID, state: camera });
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

  test('a changed fingerprint is parsed into both values', () => {
    expect(parseHostKeyChanged(`HOSTKEY_CHANGED SHA256:old ${SHA}`)).toEqual({ expected: 'SHA256:old', actual: SHA });
    expect(parseHostKeyChanged(`HOSTKEY ${SHA} ${MD5}`)).toBeNull();
    expect(parseHostKeyChanged('Cannot connect to 1.2.3.4:15740: refused')).toBeNull();
  });

  test('a changed fingerprint stops with both values and nothing to confirm', async () => {
    vi.mocked(ipc.connect).mockRejectedValueOnce(`HOSTKEY_CHANGED SHA256:old ${SHA}`);
    const store = storeWith('Automatic', {}, { reconnecting: 4, interval: start({ everyS: 5, count: null }, 0) });
    const actions = createActions(store);
    expect(await actions.connect('192.168.122.1', null, { ...login, fingerprint: 'SHA256:old' })).toBe(false);
    const pending = store.get().pendingFingerprint!;
    expect(pending.changed).toEqual({ expected: 'SHA256:old', actual: SHA });
    expect(store.get().error).toBeNull();
    expect(store.get().screen).toBe('connect');
    expect(store.get().reconnecting).toBeNull();
    expect(store.get().interval).toBeNull();
    // Nothing can be confirmed on this path: retry never connects.
    expect(await pending.retry()).toBe(false);
    expect(ipc.connect).toHaveBeenCalledTimes(1);
  });

  test('a remembered fingerprint is matched by address with or without the port', () => {
    const rc: RecentCamera = {
      ip: '10.0.0.7', model: 'ILCE-7M4', lastUsed: 1, ssid: null, sshUser: 'cam', sshFingerprint: SHA,
    };
    const settings = { ...defaultSettings('/tmp'), recentCameras: [rc] };
    expect(rememberedFingerprint(settings, '10.0.0.7:15740', null)).toBe(SHA);
    expect(rememberedFingerprint(settings, ' 10.0.0.7 ', null)).toBe(SHA);
    expect(rememberedFingerprint(settings, '10.0.0.8:15740', null)).toBeNull();
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

  test('a remembered Access Authentication camera is added beside the connected ones', async () => {
    const rc: RecentCamera = {
      ip: '192.168.122.1', model: 'ILCE-7M4', lastUsed: 1, ssid: SSID, sshUser: 'cam', sshFingerprint: SHA,
    };
    const captures = [{ path: '/p/a.JPG', filename: 'a.JPG', bytes: 1, cameraId: A, cameraLabel: 'ZV-E10' }];
    const store = storeWith('Automatic', { recentCameras: [rc] }, {
      screen: 'main', camera, cameras: { [A]: camera }, activeId: A, captures,
    });
    expect(await createActions(store).connectRecent(rc)).toBe(false);
    expect(ipc.disconnect).not.toHaveBeenCalled();
    expect(store.get().camera).toBe(camera);
    expect(store.get().cameras).toEqual({ [A]: camera });
    expect(store.get().captures).toEqual(captures);
    expect(store.get().screen).toBe('connect');
    expect(store.get().keepCameras).toBe(true);
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

describe('connect clears a stale link state', () => {
  test('reconnecting a camera clears its reconnecting and its interval; the top bar says Wi-Fi', async () => {
    const store = storeWith('Automatic', {}, {
      screen: 'main',
      camera: { ...camera, connected: false },
      cameras: { [ID]: { ...camera, connected: false } },
      activeId: ID,
      reconnecting: 5,
      attempts: { [ID]: 5 },
      interval: start({ everyS: 5, count: null }, 0, ID),
    });
    const root = document.createElement('div');
    const unmount = mountTopbar(root, store, {} as never);
    expect(await createActions(store).connect('192.168.122.1')).toBe(true);
    expect(store.get().attempts).toEqual({});
    expect(store.get().reconnecting).toBeNull();
    expect(store.get().interval).toBeNull();
    expect(root.textContent).toContain('Wi-Fi');
    expect(root.textContent).not.toContain('Reconnecting');
    unmount();
  });

  test('joining another network drops a camera reached through another network first', async () => {
    const order: string[] = [];
    vi.mocked(ipc.disconnect).mockImplementation(async () => void order.push('disconnect'));
    vi.mocked(ipc.joinNetwork).mockImplementation(async () => void order.push('join'));
    const store = storeWith('Automatic', {}, {
      screen: 'main', camera, cameras: { [SSID]: camera }, activeId: SSID, reconnecting: 2,
      attempts: { [SSID]: 2 }, interval: start({ everyS: 5, count: null }, 0, SSID),
    });
    let atJoin: Partial<AppState> | null = null;
    vi.mocked(ipc.joinNetwork).mockImplementationOnce(async () => {
      order.push('join');
      atJoin = { reconnecting: store.get().reconnecting, interval: store.get().interval, camera: store.get().camera };
    });
    expect(await createActions(store).connectWithWifi({ ssid: 'DIRECT-new:ILCE-7M4', password: 'pw' })).toBe(true);
    expect(order).toEqual(['disconnect', 'join']);
    expect(ipc.disconnect).toHaveBeenCalledWith(SSID);
    expect(atJoin).toEqual({ reconnecting: null, interval: null, camera: null });
  });

  test('a second click during the pre-join disconnect does not start a second join', async () => {
    let release: () => void = () => {};
    vi.mocked(ipc.disconnect).mockImplementation(() => new Promise<void>((r) => (release = r)));
    const store = storeWith('Automatic', {}, { screen: 'main', camera, cameras: { [SSID]: camera }, activeId: SSID });
    const actions = createActions(store);
    const first = actions.connectWithWifi({ ssid: 'DIRECT-new:ILCE-7M4', password: 'pw' });
    await Promise.resolve();
    expect(store.get().connecting).not.toBeNull();
    expect(await actions.connectWithWifi({ ssid: 'DIRECT-new:ILCE-7M4', password: 'pw' })).toBe(false);
    release();
    expect(await first).toBe(true);
    expect(ipc.disconnect).toHaveBeenCalledTimes(1);
    expect(ipc.joinNetwork).toHaveBeenCalledTimes(1);
  });

  test('joining a network keeps cameras reached by address and the one on that network', async () => {
    const store = storeWith('Automatic', {}, { screen: 'main', camera, cameras: { [A]: camera }, activeId: A });
    vi.mocked(ipc.connect).mockResolvedValue({ id: B, state: a7 });
    expect(await createActions(store).connectWithWifi({ ssid: B, password: 'pw' })).toBe(true);
    expect(ipc.disconnect).not.toHaveBeenCalled();
    expect(Object.keys(store.get().cameras)).toEqual([A, B]);
  });

  test('joining with no camera connected does not disconnect', async () => {
    const store = storeWith('Automatic');
    await createActions(store).connectWithWifi({ ssid: SSID, password: 'pw' });
    expect(ipc.disconnect).not.toHaveBeenCalled();
    expect(ipc.joinNetwork).toHaveBeenCalledTimes(1);
  });

  test('Ask mode also drops a camera on another network before waiting', async () => {
    const store = storeWith('Ask', {}, { screen: 'main', camera, cameras: { [B]: camera }, activeId: B });
    await createActions(store).connectWithWifi({ ssid: SSID, password: '' });
    expect(ipc.disconnect).toHaveBeenCalledTimes(1);
    expect(ipc.joinNetwork).not.toHaveBeenCalled();
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
        plan: { everyS: 2, count: 3 }, taken: 0, missed: 0, nextAt: 12_000, running: true, cameraId: null,
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

describe('several cameras', () => {
  const two = (extra: Partial<AppState> = {}) =>
    storeWith('Automatic', {}, { screen: 'main', camera, cameras: { [A]: camera, [B]: a7 }, activeId: A, ...extra });

  test('switchCamera makes another camera active', async () => {
    const store = two({ activePreset: 2 });
    await createActions(store).switchCamera(B);
    expect(ipc.setActiveCamera).toHaveBeenCalledWith(B);
    expect(store.get().activeId).toBe(B);
    expect(store.get().camera).toBe(a7);
    expect(store.get().activePreset).toBeNull();
    // An unknown or already active camera changes nothing.
    await createActions(store).switchCamera('nope');
    await createActions(store).switchCamera(B);
    expect(ipc.setActiveCamera).toHaveBeenCalledTimes(1);
  });

  test('an interval keeps the camera it started on after a switch', async () => {
    const store = two();
    const actions = createActions(store);
    actions.startInterval({ everyS: 5, count: null });
    expect(store.get().interval?.cameraId).toBe(A);
    await actions.switchCamera(B);
    expect(store.get().interval?.cameraId).toBe(A);
    expect(store.get().interval?.running).toBe(true);
  });

  test('shoot can name a camera', () => {
    createActions(two()).shoot(B);
    expect(ipc.shoot).toHaveBeenCalledWith(B);
  });

  test('shootAll toasts each camera that failed', async () => {
    vi.mocked(ipc.shootAll).mockResolvedValue([[A, null], [B, 'Camera busy']]);
    const toast = vi.fn();
    await createActions(two(), toast).shootAll();
    expect(toast.mock.calls).toEqual([['ILCE-7M4: Camera busy']]);
  });

  test('disconnecting the active camera activates the next one and keeps the captures', async () => {
    const captures = [{ path: '/p/a.JPG', filename: 'a.JPG', bytes: 1, cameraId: A, cameraLabel: 'ZV-E10' }];
    const store = two({
      captures,
      transfers: { [`${A}/DSC00002.JPG`]: { received: 1, total: 2 }, [`${B}/DSC00003.JPG`]: { received: 1, total: 2 } },
      interval: start({ everyS: 5, count: null }, 0, A),
    });
    await createActions(store).disconnect();
    expect(ipc.disconnect).toHaveBeenCalledWith(A);
    expect(store.get().cameras).toEqual({ [B]: a7 });
    expect(store.get().activeId).toBe(B);
    expect(store.get().camera).toBe(a7);
    expect(store.get().screen).toBe('main');
    expect(store.get().captures).toEqual(captures);
    expect(Object.keys(store.get().transfers)).toEqual([`${B}/DSC00003.JPG`]);
    expect(store.get().interval).toBeNull();
  });

  test('disconnecting another camera keeps the active one and its interval', async () => {
    const store = two({ interval: start({ everyS: 5, count: null }, 0, A) });
    await createActions(store).disconnect(B);
    expect(ipc.disconnect).toHaveBeenCalledWith(B);
    expect(store.get().activeId).toBe(A);
    expect(store.get().camera).toBe(camera);
    expect(store.get().interval?.cameraId).toBe(A);
  });

  test('disconnectAll drops every camera and shows the connect screen', async () => {
    const store = two();
    await createActions(store).disconnectAll();
    expect(ipc.disconnectAll).toHaveBeenCalledTimes(1);
    expect(store.get()).toMatchObject({ cameras: {}, activeId: null, camera: null, screen: 'connect' });
  });

  test('addCamera shows the connect screen and keeps the cameras', () => {
    const store = two({ error: 'old' });
    createActions(store).addCamera();
    expect(store.get()).toMatchObject({ screen: 'connect', keepCameras: true, activeId: A, camera, error: null });
    expect(Object.keys(store.get().cameras)).toEqual([A, B]);
  });
});

describe('card', () => {
  const jpeg = cardObject as CardObject;
  const clip: CardObject = { ...jpeg, handle: 259, filename: 'C0001.MP4', kind: 'Video', bytes: 64 << 20 };
  const browsing = (extra: Partial<AppState> = {}) =>
    createStore<AppState>({
      ...initialState(),
      screen: 'main',
      camera,
      cameras: { [A]: camera, [B]: a7 },
      activeId: A,
      cardOpen: true,
      ...extra,
    });

  beforeEach(() => {
    vi.mocked(ipc.cardMode).mockReset().mockResolvedValue('same-session');
    vi.mocked(ipc.cardList).mockReset().mockResolvedValue([jpeg, clip]);
    vi.mocked(ipc.cardDownload).mockReset().mockResolvedValue('/shots/C0001.MP4');
  });

  test('refresh lists the active camera card', async () => {
    const store = browsing();
    const pending = createActions(store).cardRefresh();
    expect(store.get().card.loading).toBe(true);
    await pending;
    expect(ipc.cardList).toHaveBeenCalledWith(A);
    expect(store.get().card).toMatchObject({ items: [jpeg, clip], loading: false, error: null, mode: 'same-session' });
  });

  test('a camera that needs its transfer mode is not listed', async () => {
    vi.mocked(ipc.cardMode).mockResolvedValue('needs-transfer-mode');
    const store = browsing();
    await createActions(store).cardRefresh();
    expect(ipc.cardList).not.toHaveBeenCalled();
    expect(store.get().card).toMatchObject({ items: [], loading: false, mode: 'needs-transfer-mode' });
  });

  test('a failed listing is shown in the panel', async () => {
    vi.mocked(ipc.cardList).mockRejectedValue('Not connected to a camera');
    const toast = vi.fn();
    const store = browsing();
    await createActions(store, toast).cardRefresh();
    expect(store.get().card).toMatchObject({ loading: false, error: 'Not connected to a camera' });
    expect(toast).not.toHaveBeenCalled();
  });

  test('a listing that arrives after switching cameras is dropped', async () => {
    let finish: (items: CardObject[]) => void = () => {};
    vi.mocked(ipc.cardList).mockReturnValue(new Promise((r) => (finish = r)));
    const store = browsing();
    const actions = createActions(store);
    const pending = actions.cardRefresh();
    await vi.waitFor(() => expect(ipc.cardList).toHaveBeenCalled());
    await actions.switchCamera(B);
    finish([jpeg, clip]);
    await pending;
    expect(store.get().card).toEqual(emptyCard());
  });

  test('download shows progress at once, then the saved path', async () => {
    let finish: (path: string) => void = () => {};
    vi.mocked(ipc.cardDownload).mockReturnValue(new Promise((r) => (finish = r)));
    const store = browsing({ card: { ...emptyCard(), items: [jpeg, clip] } });
    const pending = createActions(store).cardDownload(259);
    expect(ipc.cardDownload).toHaveBeenCalledWith(259, A);
    expect(store.get().card.downloads).toEqual({ 259: { received: 0, total: 64 << 20 } });
    finish('/shots/C0001.MP4');
    await pending;
    expect(store.get().card.downloads).toEqual({});
    expect(store.get().card.done).toEqual({ 259: '/shots/C0001.MP4' });
  });

  test('only the reply marks the row saved; progress after it is ignored', async () => {
    let finish: (path: string) => void = () => {};
    vi.mocked(ipc.cardDownload).mockReturnValue(new Promise((r) => (finish = r)));
    const store = browsing({ card: { ...emptyCard(), items: [jpeg, clip] } });
    const route = (event: Parameters<typeof applyEvent>[1]) => store.set(applyEvent(store.get(), event, A));
    const pending = createActions(store).cardDownload(259);
    route({ CardDownloadProgress: { handle: 259, filename: 'C0001.MP4', received: 1 << 20, total: 64 << 20 } });
    expect(store.get().card.downloads).toEqual({ 259: { received: 1 << 20, total: 64 << 20 } });
    route({ CaptureSaved: { path: '/shots/C0001.MP4', bytes: 64 << 20, filename: 'C0001.MP4', camera_filename: 'C0001.MP4' } });
    expect(store.get().card.done).toEqual({});
    finish('/shots/C0001.MP4');
    await pending;
    expect(store.get().card.done).toEqual({ 259: '/shots/C0001.MP4' });
    route({ CardDownloadProgress: { handle: 259, filename: 'C0001.MP4', received: 64 << 20, total: 64 << 20 } });
    expect(store.get().card.downloads).toEqual({});
  });

  test('a file already on its way is not asked for twice', async () => {
    const store = browsing({ card: { ...emptyCard(), items: [clip], downloads: { 259: { received: 1, total: 2 } } } });
    await createActions(store).cardDownload(259);
    expect(ipc.cardDownload).not.toHaveBeenCalled();
  });

  test('a refused download keeps its error on the row, not in a toast', async () => {
    vi.mocked(ipc.cardDownload).mockRejectedValue('unsupported: files of 4 GiB or more cannot be downloaded yet');
    const toast = vi.fn();
    const store = browsing({ card: { ...emptyCard(), items: [jpeg, clip], failed: { 257: 'old' } } });
    await createActions(store, toast).cardDownload(259);
    expect(store.get().card.downloads).toEqual({});
    expect(store.get().card.failed).toEqual({
      257: 'old',
      259: 'unsupported: files of 4 GiB or more cannot be downloaded yet',
    });
    expect(toast).not.toHaveBeenCalled();
  });

  test('disconnecting the last camera closes the card', async () => {
    const store = browsing({ cameras: { [A]: camera }, card: { ...emptyCard(), items: [jpeg] } });
    await createActions(store).disconnect();
    expect(store.get().cardOpen).toBe(false);
    expect(store.get().card).toEqual(emptyCard());
  });
});

describe('join via Bluetooth', () => {
  const zv: ipc.Found = {
    address: 'fake-zv-e10',
    advert: { name: 'ZV-E10', model_code: 'E1', pairing_open: true, address: 'fake-zv-e10' },
  };

  test('reads the Wi-Fi details over Bluetooth, then joins and connects', async () => {
    const store = storeWith('Automatic');
    const texts: string[] = [];
    store.subscribe((s) => s.connecting && texts.push(s.connecting.text));
    expect(await createActions(store).joinViaBluetooth(zv)).toBe(true);
    expect(texts[0]).toBe('Reading Wi-Fi details from ZV-E10 over Bluetooth');
    expect(ipc.bleHandoff).toHaveBeenCalledWith('fake-zv-e10');
    expect(ipc.joinNetwork).toHaveBeenCalledWith(SSID, '12345678');
    expect(ipc.connect).toHaveBeenCalledWith('192.168.122.1:15740', SSID, null);
    expect(store.get().screen).toBe('main');
  });

  test('a failed read is remembered for that camera and nothing is joined', async () => {
    vi.mocked(ipc.bleHandoff).mockRejectedValue('Bluetooth failed: timed out after 5000 ms waiting to connect');
    const store = storeWith('Automatic');
    expect(await createActions(store).joinViaBluetooth(zv)).toBe(false);
    expect(ipc.joinNetwork).not.toHaveBeenCalled();
    expect(store.get().connecting).toBeNull();
    expect(store.get().bleFailed).toEqual({
      'fake-zv-e10':
        'Rimix could not read the Wi-Fi details over Bluetooth (timed out after 5000 ms waiting to connect). Scan the QR code or type the name and password instead.',
    });
  });

  test('unavailable Bluetooth is said once and stops the Bluetooth list', async () => {
    vi.mocked(ipc.bleHandoff).mockRejectedValue('Bluetooth unavailable: Bluetooth is turned off');
    const store = storeWith('Automatic', {}, { bleNearby: [zv] });
    expect(await createActions(store).joinViaBluetooth(zv)).toBe(false);
    expect(store.get().bleUnavailable).toBe(true);
    expect(store.get().bleNearby).toEqual([]);
    expect(store.get().bleFailed).toEqual({});
    expect(store.get().connecting).toBeNull();
  });

  test('does nothing while another connect runs', async () => {
    const store = storeWith('Automatic', {}, { connecting: { phase: 'joining', text: 'Joining X' } });
    expect(await createActions(store).joinViaBluetooth(zv)).toBe(false);
    expect(ipc.bleHandoff).not.toHaveBeenCalled();
  });
});
