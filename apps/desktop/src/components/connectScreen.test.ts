import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import type { Actions } from '../actions';
import { invoke } from '@tauri-apps/api/core';
import { NOT_A_WIFI_CODE, WEBCAM_OFF } from '../copy';
import type { AppSettings, CameraState, Found, RecentCamera } from '../ipc';
import { createStore, defaultSettings, initialState, type AppStore } from '../state';
import { createScanner, webcamSupported } from '../qr/scanner';
import { mount, SSH_LOGIN_NEEDED, SSH_PASSWORD_NEEDED } from './connectScreen';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => Promise.resolve([])) }));
vi.mock('../qr/scanner', () => ({
  webcamSupported: vi.fn(() => false),
  createScanner: vi.fn(() => ({ start: vi.fn(), stop: vi.fn() })),
}));

function fakeActions(): Actions {
  return {
    connect: vi.fn(() => Promise.resolve(true)),
    connectWithWifi: vi.fn(() => Promise.resolve(true)),
    connectRecent: vi.fn(() => Promise.resolve(true)),
    forgetCamera: vi.fn(() => Promise.resolve()),
    joinViaBluetooth: vi.fn(() => Promise.resolve(true)),
  } as unknown as Actions;
}

let root: HTMLElement;
let store: AppStore;
let actions: Actions;
let unmount: () => void;

function setup(settings: Partial<AppSettings> = {}, webcam = true) {
  document.body.innerHTML = '';
  root = document.createElement('section');
  document.body.append(root);
  store = createStore({ ...initialState(), settings: { ...defaultSettings('/tmp'), ...settings }, webcam });
  actions = fakeActions();
  unmount = mount(root, store, actions);
}

const message = () => root.querySelector<HTMLElement>('.headline .message')!;
const byText = (text: string) =>
  [...root.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent === text)!;

describe('connect screen', () => {
  beforeEach(() => setup());
  afterEach(() => unmount());

  test('shows three ways with the address field behind Advanced', () => {
    expect([...root.querySelectorAll('.way h2')].map((h) => h.textContent)).toEqual([
      'Scan the QR code',
      'Nearby',
      'Type the Wi-Fi name and password',
    ]);
    const adv = byText('Advanced: connect by address');
    expect(adv.getAttribute('aria-expanded')).toBe('false');
    expect(root.querySelector('#ip')!.closest('[hidden]')).not.toBeNull();
    adv.click();
    expect(adv.getAttribute('aria-expanded')).toBe('true');
    expect(root.querySelector('#ip')!.closest('[hidden]')).toBeNull();
    expect(root.querySelector<HTMLInputElement>('#ip')!.value).toBe('192.168.122.1');
  });

  test('the scan card has a preview with video and a hidden canvas', () => {
    const preview = root.querySelector('.preview')!;
    const video = preview.querySelector('video')!;
    expect(video.muted).toBe(true);
    expect(video.hasAttribute('playsinline')).toBe(true);
    expect(preview.querySelector('canvas')!.hidden).toBe(true);
    // No webcam support here, so the status line says so.
    expect(root.textContent).toContain("The webcam isn't available here.");
  });

  test('typing a name and password connects over Wi-Fi', () => {
    root.querySelector<HTMLInputElement>('#ssid')!.value = 'DIRECT-abc1:ZV-E10';
    root.querySelector<HTMLInputElement>('#pw')!.value = '12345678';
    root.querySelector<HTMLInputElement>('#pw')!.closest('form')!.requestSubmit();
    expect(actions.connectWithWifi).toHaveBeenCalledWith({ ssid: 'DIRECT-abc1:ZV-E10', password: '12345678' }, null);
  });

  test('Show reveals the password', () => {
    const pw = root.querySelector<HTMLInputElement>('#pw')!;
    expect(pw.type).toBe('password');
    const show = byText('Show');
    show.click();
    expect(pw.type).toBe('text');
    expect(show.textContent).toBe('Hide');
    show.click();
    expect(pw.type).toBe('password');
  });

  test('the address under Advanced connects as before', () => {
    byText('Advanced: connect by address').click();
    root.querySelector<HTMLInputElement>('#ip')!.closest('form')!.requestSubmit();
    expect(actions.connect).toHaveBeenCalledWith('192.168.122.1', null, null);
  });

  test('pasted text that is not a Wi-Fi code is explained and left for editing', () => {
    byText("Paste the code's text").click();
    const area = root.querySelector('textarea')!;
    expect(area.closest('[hidden]')).toBeNull();
    area.value = 'https://sony.net';
    byText('Use this text').click();
    expect(message().textContent).toBe(NOT_A_WIFI_CODE);
    expect(area.value).toBe('https://sony.net');
    expect(area.closest('[hidden]')).toBeNull();
    expect(document.activeElement).toBe(root.querySelector('#ssid'));
    expect(actions.connectWithWifi).not.toHaveBeenCalled();
  });

  test('pasted Sony text connects', () => {
    byText("Paste the code's text").click();
    root.querySelector('textarea')!.value = 'W01:S:DIRECT-abc1:ZV-E10;P:12345678;C:ZV-E10;';
    byText('Use this text').click();
    expect(actions.connectWithWifi).toHaveBeenCalledWith(
      expect.objectContaining({ ssid: 'DIRECT-abc1:ZV-E10', password: '12345678' }),
      null,
    );
  });

  test('disables Connect buttons and shows progress while connecting', () => {
    store.set({ connecting: { phase: 'joining', text: 'Joining X' } });
    const buttons = [...root.querySelectorAll<HTMLButtonElement>('.pbtn')];
    expect(buttons.length).toBeGreaterThan(0);
    expect(buttons.every((b) => b.disabled)).toBe(true);
    expect(message().textContent).toBe('Joining X');
    expect(message().classList.contains('error')).toBe(false);

    store.set({ connecting: null, error: 'Something failed' });
    expect(buttons.every((b) => !b.disabled)).toBe(true);
    expect(message().textContent).toBe('Something failed');
    expect(message().classList.contains('error')).toBe(true);
  });
});

describe('Access Authentication', () => {
  beforeEach(() => setup());
  afterEach(() => unmount());

  const SHA = 'SHA256:q1w2e3r4t5y6u7i8o9p0asdfghjklzxcvbnm1234567';
  const disclosure = () => byText('This camera uses Access Authentication');
  const user = () => root.querySelector<HTMLInputElement>('#ssh-user')!;
  const pass = () => root.querySelector<HTMLInputElement>('#ssh-pw')!;
  const nearbyCamera = (ssh: boolean | null) => ({
    model: 'ILCE-7M4',
    friendly_name: 'ILCE-7M4',
    ip: '192.168.1.20',
    ptp_port: 15740,
    ptp_version: null,
    pairing_required: null,
    ssh_supported: ssh,
    source: 'Ssdp' as const,
  });

  test('sits closed under the name and password, above Advanced', () => {
    const d = disclosure();
    expect(d.getAttribute('aria-expanded')).toBe('false');
    const order = [...root.querySelectorAll('button[aria-expanded]')].map((b) => b.textContent);
    expect(order).toEqual(['This camera uses Access Authentication', 'Advanced: connect by address']);
    expect(user().closest('[hidden]')).not.toBeNull();
    expect(pass().type).toBe('password');
    d.click();
    expect(d.getAttribute('aria-expanded')).toBe('true');
    expect(user().closest('[hidden]')).toBeNull();
    expect(root.querySelector('label[for="ssh-user"]')!.textContent).toBe('Username');
    expect(root.querySelector('label[for="ssh-pw"]')!.textContent).toBe('Password');
  });

  test('its Show reveals the password', () => {
    disclosure().click();
    const show = pass().parentElement!.querySelector<HTMLButtonElement>('button')!;
    expect(show.textContent).toBe('Show');
    show.click();
    expect(pass().type).toBe('text');
  });

  test('an open login with a username goes with every connect', () => {
    disclosure().click();
    user().value = 'cam';
    pass().value = '12345678';
    const login = { user: 'cam', password: '12345678', fingerprint: null };
    root.querySelector<HTMLInputElement>('#ssid')!.value = 'DIRECT-zz:ILCE-7M4';
    root.querySelector<HTMLInputElement>('#pw')!.closest('form')!.requestSubmit();
    expect(actions.connectWithWifi).toHaveBeenCalledWith({ ssid: 'DIRECT-zz:ILCE-7M4', password: '' }, login);
    byText('Advanced: connect by address').click();
    root.querySelector<HTMLInputElement>('#ip')!.closest('form')!.requestSubmit();
    expect(actions.connect).toHaveBeenCalledWith('192.168.122.1', null, login);
    // Closed again, the login is left out.
    disclosure().click();
    root.querySelector<HTMLInputElement>('#ip')!.closest('form')!.requestSubmit();
    expect(actions.connect).toHaveBeenLastCalledWith('192.168.122.1', null, null);
  });

  test('a nearby camera that supports it opens the login before connecting', () => {
    store.set({ nearby: [nearbyCamera(true)] });
    const connect = () => root.querySelector<HTMLButtonElement>('.device .pbtn')!;
    connect().click();
    expect(disclosure().getAttribute('aria-expanded')).toBe('true');
    expect(actions.connect).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(user());
    user().value = 'cam';
    pass().value = 'pw';
    connect().click();
    expect(actions.connect).toHaveBeenCalledWith('192.168.1.20', null, {
      user: 'cam',
      password: 'pw',
      fingerprint: null,
    });
  });

  test('a nearby camera without it connects at once', () => {
    store.set({ nearby: [nearbyCamera(null)] });
    root.querySelector<HTMLButtonElement>('.device .pbtn')!.click();
    expect(actions.connect).toHaveBeenCalledWith('192.168.1.20', null, null);
    expect(disclosure().getAttribute('aria-expanded')).toBe('false');
  });

  test('a remembered camera fills the username and waits for the password', () => {
    store.set({ sshPrefill: { user: 'cam', ssid: 'DIRECT-zz:ILCE-7M4', ip: '192.168.122.1' } });
    expect(disclosure().getAttribute('aria-expanded')).toBe('true');
    expect(user().value).toBe('cam');
    expect(pass().value).toBe('');
    expect(root.querySelector<HTMLInputElement>('#ssid')!.value).toBe('DIRECT-zz:ILCE-7M4');
    expect(document.activeElement).toBe(pass());
    expect(actions.connectWithWifi).not.toHaveBeenCalled();

    // Known only by address: the address is filled under Advanced.
    store.set({ sshPrefill: { user: 'ops', ssid: null, ip: '10.0.0.7' } });
    expect(byText('Advanced: connect by address').getAttribute('aria-expanded')).toBe('true');
    expect(root.querySelector<HTMLInputElement>('#ip')!.value).toBe('10.0.0.7');
    expect(user().value).toBe('ops');
  });

  test('a fingerprint to confirm is shown until confirmed or cancelled', () => {
    const block = () => root.querySelector<HTMLElement>('.fingerprint')!;
    expect(block().hidden).toBe(true);
    const retry = vi.fn(() => Promise.resolve(true));
    store.set({ pendingFingerprint: { sha256: SHA, md5: 'aa:bb', retry } });
    expect(block().hidden).toBe(false);
    expect(block().querySelector('p')!.textContent).toBe(
      `This camera's fingerprint is ${SHA} (MD5 aa:bb). Check it against Access Authen. Info on the camera; it may show either form.`,
    );
    byText('It matches, connect').click();
    expect(retry).toHaveBeenCalledTimes(1);
    byText('Cancel').click();
    expect(store.get().pendingFingerprint).toBeNull();
    expect(block().hidden).toBe(true);
  });

  test('a changed fingerprint shows both values and only Cancel', () => {
    const block = () => root.querySelector<HTMLElement>('.fingerprint')!;
    const OLD = 'SHA256:storedstoredstoredstoredstoredstoredstor';
    store.set({
      pendingFingerprint: {
        sha256: SHA,
        md5: '',
        retry: vi.fn(() => Promise.resolve(false)),
        changed: { expected: OLD, actual: SHA },
      },
    });
    expect(block().hidden).toBe(false);
    const text = block().querySelector('p')!.textContent!;
    expect(text).toBe(
      `This camera's fingerprint changed. Stored: ${OLD}. Camera now: ${SHA}. If you reset the camera, forget it in App settings and connect again; otherwise do not connect.`,
    );
    expect(byText('It matches, connect').hidden).toBe(true);
    byText('Cancel').click();
    expect(store.get().pendingFingerprint).toBeNull();

    // An ordinary confirmation afterwards offers the connect button again.
    store.set({ pendingFingerprint: { sha256: SHA, md5: 'aa:bb', retry: vi.fn(() => Promise.resolve(true)) } });
    expect(byText('It matches, connect').hidden).toBe(false);
  });

  test('an open login without a password asks for it instead of connecting', () => {
    disclosure().click();
    user().value = 'cam';
    root.querySelector<HTMLInputElement>('#ssid')!.value = 'DIRECT-zz:ILCE-7M4';
    root.querySelector<HTMLInputElement>('#pw')!.closest('form')!.requestSubmit();
    expect(actions.connectWithWifi).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(pass());
    expect(message().textContent).toBe(SSH_PASSWORD_NEEDED);
    expect(SSH_PASSWORD_NEEDED).toBe('Enter the password shown under Access Authen. Info.');

    byText('Advanced: connect by address').click();
    root.querySelector<HTMLInputElement>('#ip')!.closest('form')!.requestSubmit();
    expect(actions.connect).not.toHaveBeenCalled();

    pass().value = 'pw';
    root.querySelector<HTMLInputElement>('#ip')!.closest('form')!.requestSubmit();
    expect(actions.connect).toHaveBeenCalledWith('192.168.122.1', null, { user: 'cam', password: 'pw', fingerprint: null });
  });
});

describe('Access Authentication with nothing typed', () => {
  beforeEach(() => setup());
  afterEach(() => unmount());

  test('an open login with no username and no password names the username field', () => {
    byText('This camera uses Access Authentication').click();
    root.querySelector<HTMLInputElement>('#ssid')!.value = 'DIRECT-zz:ILCE-7M4';
    root.querySelector<HTMLInputElement>('#pw')!.closest('form')!.requestSubmit();
    expect(actions.connectWithWifi).not.toHaveBeenCalled();
    expect(message().textContent).toBe(SSH_LOGIN_NEEDED);
    expect(SSH_LOGIN_NEEDED).toBe('Enter the username and password shown under Access Authen. Info.');
    expect(document.activeElement).toBe(root.querySelector('#ssh-user'));
  });
});

describe('recently used', () => {
  afterEach(() => unmount());

  test('recent cameras show the network and connect through it', () => {
    const rc: RecentCamera = { ip: '192.168.122.1', model: 'ZV-E10', ssid: 'DIRECT-abc1:ZV-E10', lastUsed: 1 };
    setup({ recentCameras: [rc] });
    const row = root.querySelector<HTMLButtonElement>('.recent button')!;
    expect(row.querySelector('.k')!.textContent).toBe('ZV-E10');
    expect(row.querySelector('.v')!.textContent).toBe('DIRECT-abc1:ZV-E10');
    row.click();
    expect(actions.connectRecent).toHaveBeenCalledWith(rc);

    store.set({ connecting: { phase: 'finding', text: 'Looking' } });
    expect(root.querySelector<HTMLButtonElement>('.recent button')!.disabled).toBe(true);
  });

  test('a camera without a network shows its address', () => {
    setup({ recentCameras: [{ ip: '10.0.0.5', model: 'A7 IV', ssid: null, lastUsed: 1 }] });
    expect(root.querySelector('.recent .v')!.textContent).toBe('10.0.0.5');
  });
});

describe('scanning', () => {
  afterEach(() => {
    unmount();
    vi.useRealTimers();
    vi.mocked(webcamSupported).mockReturnValue(false);
  });

  test('a code that is not a Wi-Fi code is explained and scanning carries on', async () => {
    vi.useFakeTimers();
    vi.mocked(webcamSupported).mockReturnValue(true);
    const start = vi.fn(() => Promise.resolve());
    let report: (text: string) => void = () => {};
    vi.mocked(createScanner).mockImplementation((opts) => {
      report = opts.onResult;
      return { start, stop: vi.fn() };
    });
    setup();
    expect(start).toHaveBeenCalledTimes(1);

    report('https://sony.net/app');
    await vi.advanceTimersByTimeAsync(0);
    expect(message().textContent).toBe(NOT_A_WIFI_CODE);
    const area = root.querySelector('textarea')!;
    expect(area.value).toBe('https://sony.net/app');
    expect(area.closest('[hidden]')).toBeNull();
    expect(document.activeElement).toBe(root.querySelector('#ssid'));
    expect(actions.connectWithWifi).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(3000);
    expect(start).toHaveBeenCalledTimes(2);

    // The user edits the text; another unrelated code leaves it and the focus alone.
    area.value = 'W01:S:DIRECT-ab';
    area.focus();
    report('https://example.com');
    await vi.advanceTimersByTimeAsync(0);
    expect(area.value).toBe('W01:S:DIRECT-ab');
    expect(document.activeElement).toBe(area);
    await vi.advanceTimersByTimeAsync(3000);
    expect(start).toHaveBeenCalledTimes(3);
  });

  test('a stray code in view does not replace progress or a failed connect', async () => {
    vi.useFakeTimers();
    vi.mocked(webcamSupported).mockReturnValue(true);
    const start = vi.fn(() => Promise.resolve());
    let report: (text: string) => void = () => {};
    vi.mocked(createScanner).mockImplementation((opts) => {
      report = opts.onResult;
      return { start, stop: vi.fn() };
    });
    setup();

    // Read while a typed connect runs: progress stays, no error is left behind.
    store.set({ connecting: { phase: 'finding', text: 'Looking for the camera on X' } });
    report('https://sony.net/app');
    await vi.advanceTimersByTimeAsync(0);
    expect(message().textContent).toBe('Looking for the camera on X');
    expect(store.get().error).toBeNull();
    await vi.advanceTimersByTimeAsync(3000);
    expect(start).toHaveBeenCalledTimes(2);

    // The connect fails; the next read of the same code leaves the failure in place.
    const failure = 'Joined X but no camera answered. On the camera, open Menu, Network, PC Remote Function, and stay on that screen.';
    store.set({ connecting: null, error: failure });
    report('https://sony.net/app');
    await vi.advanceTimersByTimeAsync(3000);
    expect(message().textContent).toBe(failure);
    expect(start).toHaveBeenCalledTimes(3);
  });

  function scanning() {
    vi.useFakeTimers();
    vi.mocked(webcamSupported).mockReturnValue(true);
    const start = vi.fn(() => Promise.resolve());
    let report: (text: string) => void = () => {};
    vi.mocked(createScanner).mockImplementation((opts) => {
      report = opts.onResult;
      return { start, stop: vi.fn() };
    });
    return { start, report: (t: string) => report(t) };
  }

  const SONY = 'W01:S:DIRECT-abc1:ZV-E10;P:12345678;C:ZV-E10;';
  const OTHER = 'W01:S:DIRECT-xyz9:A7IV;P:87654321;C:ILCE-7M4;';
  const FAILURE = 'Joined DIRECT-abc1:ZV-E10 but no camera answered.';

  test('a scanned code fills the fields and the text box', async () => {
    const { report } = scanning();
    setup();
    report(SONY);
    await vi.advanceTimersByTimeAsync(0);
    expect(root.querySelector<HTMLInputElement>('#ssid')!.value).toBe('DIRECT-abc1:ZV-E10');
    expect(root.querySelector<HTMLInputElement>('#pw')!.value).toBe('12345678');
    const area = root.querySelector('textarea')!;
    expect(area.value).toBe(SONY);
    expect(area.closest('[hidden]')).toBeNull();
  });

  test('a failed scanned connect is not retried by reading the same code again', async () => {
    const { start, report } = scanning();
    setup();
    vi.mocked(actions.connectWithWifi).mockImplementation(async () => {
      store.set({ connecting: null, error: FAILURE });
      return false;
    });
    expect(byText('Scan again').hidden).toBe(true);

    report(SONY);
    await vi.advanceTimersByTimeAsync(0);
    expect(actions.connectWithWifi).toHaveBeenCalledTimes(1);
    expect(message().textContent).toBe(FAILURE);
    expect(byText('Scan again').hidden).toBe(false);

    await vi.advanceTimersByTimeAsync(3000);
    expect(start).toHaveBeenCalledTimes(2);
    report(SONY);
    await vi.advanceTimersByTimeAsync(3000);
    expect(actions.connectWithWifi).toHaveBeenCalledTimes(1);
    expect(message().textContent).toBe(FAILURE);
    expect(start).toHaveBeenCalledTimes(3);

    // A different code connects.
    report(OTHER);
    await vi.advanceTimersByTimeAsync(0);
    expect(actions.connectWithWifi).toHaveBeenCalledTimes(2);
    expect(actions.connectWithWifi).toHaveBeenLastCalledWith(expect.objectContaining({ ssid: 'DIRECT-xyz9:A7IV' }), null);
  });

  test('Scan again lets the same code connect again', async () => {
    const { start, report } = scanning();
    setup();
    vi.mocked(actions.connectWithWifi).mockResolvedValue(false);
    report(SONY);
    await vi.advanceTimersByTimeAsync(0);
    expect(actions.connectWithWifi).toHaveBeenCalledTimes(1);

    const again = byText('Scan again');
    again.click();
    expect(again.hidden).toBe(true);
    expect(start).toHaveBeenCalledTimes(2);
    report(SONY);
    await vi.advanceTimersByTimeAsync(0);
    expect(actions.connectWithWifi).toHaveBeenCalledTimes(2);
  });

  test('a scanned code with the login open and no password does not scan again', async () => {
    const { start, report } = scanning();
    setup();
    byText('This camera uses Access Authentication').click();
    root.querySelector<HTMLInputElement>('#ssh-user')!.value = 'cam';
    report(SONY);
    await vi.advanceTimersByTimeAsync(0);
    expect(actions.connectWithWifi).not.toHaveBeenCalled();
    expect(message().textContent).toBe(SSH_PASSWORD_NEEDED);
    expect(document.activeElement).toBe(root.querySelector('#ssh-pw'));
    await vi.advanceTimersByTimeAsync(10_000);
    expect(start).toHaveBeenCalledTimes(1);
    // The fields hold the code; Scan again is there to look once more.
    expect(root.querySelector<HTMLInputElement>('#ssid')!.value).toBe('DIRECT-abc1:ZV-E10');
    expect(byText('Scan again').hidden).toBe(false);
  });

  test('with the webcam off for this run the scanner never starts', async () => {
    const { start } = scanning();
    setup({ webcamId: 'cam-a' }, false);
    expect(start).not.toHaveBeenCalled();
    expect(root.querySelector('.scanstatus')!.textContent).toBe(WEBCAM_OFF);
    expect(WEBCAM_OFF).toBe('Webcam off for this run');
    store.set({ settings: { ...store.get().settings!, webcamId: 'cam-b' } });
    expect(start).not.toHaveBeenCalled();
    expect(root.querySelector('.scanstatus')!.textContent).toBe(WEBCAM_OFF);
  });

  test('the scanner follows the webcam chosen in App settings', async () => {
    const { start } = scanning();
    setup({ webcamId: 'cam-a' });
    expect(vi.mocked(createScanner).mock.lastCall![0].deviceId).toBe('cam-a');
    expect(start).toHaveBeenCalledTimes(1);
    store.set({ settings: { ...store.get().settings!, webcamId: 'cam-b' } });
    expect(vi.mocked(createScanner).mock.lastCall![0].deviceId).toBe('cam-b');
    expect(start).toHaveBeenCalledTimes(2);
    // Other settings changes leave the scanner alone.
    store.set({ settings: { ...store.get().settings!, appearance: 'Light' } });
    expect(start).toHaveBeenCalledTimes(2);
  });
});

describe('adding a camera', () => {
  test('Back returns to the main screen while cameras stay connected', () => {
    setup();
    const back = () => root.querySelector<HTMLButtonElement>('.headline .back')!;
    expect(back().hidden).toBe(true);
    const camera = { model: 'ZV-E10' } as unknown as CameraState;
    store.set({ cameras: { a: camera }, activeId: 'a', camera, keepCameras: true });
    expect(back().hidden).toBe(false);
    expect(back().textContent).toBe('Back to ZV-E10');
    back().click();
    expect(store.get().screen).toBe('main');
    expect(store.get().keepCameras).toBe(false);
    unmount();
  });
});

describe('Bluetooth in Nearby', () => {
  const zv: Found = {
    address: 'fake-zv-e10',
    advert: { name: 'ZV-E10', model_code: 'E1', pairing_open: true, address: 'fake-zv-e10' },
  };
  const unnamed: Found = {
    address: 'cam-2',
    advert: { name: null, model_code: 'E2', pairing_open: false, address: 'cam-2' },
  };
  const a7 = {
    model: 'ILCE-7M4',
    friendly_name: 'ILCE-7M4',
    ip: '192.168.1.20',
    ptp_port: 15740,
    ptp_version: null,
    pairing_required: null,
    ssh_supported: null,
    source: 'Ssdp' as const,
  };
  const UNAVAILABLE = 'Bluetooth is off or not allowed; nearby cameras come from Wi-Fi only.';
  let bleAnswer: () => Promise<unknown>;
  const scans = () => vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === 'ble_scan');
  const rows = () => [...root.querySelectorAll<HTMLElement>('.devices .device')];
  const flush = () => vi.advanceTimersByTimeAsync(0);

  beforeEach(() => {
    vi.useFakeTimers();
    bleAnswer = () => Promise.resolve([zv]);
    vi.mocked(invoke).mockReset().mockImplementation((cmd: string) =>
      cmd === 'ble_scan' ? bleAnswer() : cmd === 'discover' ? Promise.resolve([a7]) : Promise.resolve([]),
    );
  });
  afterEach(() => {
    unmount();
    vi.useRealTimers();
    vi.mocked(invoke).mockReset().mockImplementation(() => Promise.resolve([]));
  });

  test('cameras heard over Bluetooth are listed under the Wi-Fi ones', async () => {
    bleAnswer = () => Promise.resolve([zv, unnamed]);
    setup();
    await flush();
    expect(scans()[0]).toEqual(['ble_scan', { seconds: 3 }]);
    expect(rows().map((r) => r.querySelector('.t b')!.textContent)).toEqual([
      'ILCE-7M4',
      'ZV-E10 nearby',
      'Sony camera E2 nearby',
    ]);
    expect(rows()[1].querySelector('.t span')!.textContent).toBe('pairing open');
    expect(rows()[2].querySelector('.t span')!.textContent).toBe('pairing off');
    const join = rows()[1].querySelector('button')!;
    expect(join.textContent).toBe('Join via Bluetooth');
    join.click();
    expect(actions.joinViaBluetooth).toHaveBeenCalledWith(zv);
  });

  test('Join via Bluetooth waits while a connect runs', async () => {
    setup();
    await flush();
    store.set({ connecting: { phase: 'joining', text: 'Joining X' } });
    expect(rows()[1].querySelector('button')!.disabled).toBe(true);
    store.set({ connecting: null });
    expect(rows()[1].querySelector('button')!.disabled).toBe(false);
  });

  test('scans every 10 s, never two at once, and stops on unmount', async () => {
    let finish: (v: Found[]) => void = () => {};
    bleAnswer = () => new Promise((resolve) => (finish = resolve));
    setup();
    await flush();
    expect(scans()).toHaveLength(1);
    // The first scan is still running: no second one starts.
    await vi.advanceTimersByTimeAsync(25_000);
    expect(scans()).toHaveLength(1);
    finish([zv]);
    await flush();
    expect(scans()).toHaveLength(2);
    finish([zv]);
    await vi.advanceTimersByTimeAsync(9_000);
    expect(scans()).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(scans()).toHaveLength(3);
    finish([zv]);
    await flush();
    unmount();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(scans()).toHaveLength(3);
    unmount = () => {};
  });

  test('unavailable Bluetooth is said once, the Wi-Fi cameras stay and nothing retries', async () => {
    bleAnswer = () => Promise.reject('Bluetooth unavailable: this app may not use Bluetooth');
    setup();
    await flush();
    const quiet = [...root.querySelectorAll('.devices p')].filter((p) => p.textContent === UNAVAILABLE);
    expect(quiet).toHaveLength(1);
    expect(rows().map((r) => r.querySelector('.t b')!.textContent)).toEqual(['ILCE-7M4']);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(scans()).toHaveLength(1);
    expect([...root.querySelectorAll('.devices p')].filter((p) => p.textContent === UNAVAILABLE)).toHaveLength(1);
    // With no Wi-Fi camera either, the line still stands beside the looking line.
    store.set({ nearby: [] });
    expect(root.querySelector('.devices')!.textContent).toContain(UNAVAILABLE);
  });

  test('another error leaves the list as it was and scans again later', async () => {
    setup();
    await flush();
    bleAnswer = () => Promise.reject('Bluetooth failed: timed out after 5000 ms waiting to start scanning');
    await vi.advanceTimersByTimeAsync(10_000);
    expect(scans()).toHaveLength(2);
    expect(rows()).toHaveLength(2);
    expect(root.querySelector('.devices')!.textContent).not.toContain(UNAVAILABLE);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(scans()).toHaveLength(3);
  });

  test('with Bluetooth off in App settings nothing is scanned', async () => {
    setup({ bluetooth: false });
    await vi.advanceTimersByTimeAsync(30_000);
    expect(scans()).toHaveLength(0);
    expect(rows()).toHaveLength(1);
    // Turned on while the screen is up, scanning starts.
    store.set({ settings: { ...store.get().settings!, bluetooth: true } });
    await flush();
    expect(scans()).toHaveLength(1);
    expect(rows()).toHaveLength(2);
    store.set({ settings: { ...store.get().settings!, bluetooth: false } });
    expect(rows()).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(scans()).toHaveLength(1);
  });

  test("a failed read shows under its camera once, until the list changes", async () => {
    setup();
    await flush();
    const failure =
      'Rimix could not read the Wi-Fi details over Bluetooth (timed out). Scan the QR code or type the name and password instead.';
    store.set({ bleFailed: { [zv.address]: failure } });
    const lines = () => [...root.querySelectorAll('.devices p')].filter((p) => p.textContent === failure);
    expect(lines()).toHaveLength(1);
    // The same camera heard again: the line stays, once.
    await vi.advanceTimersByTimeAsync(10_000);
    expect(lines()).toHaveLength(1);
    // Another camera appears: the list changed, so the failure is forgotten.
    bleAnswer = () => Promise.resolve([zv, unnamed]);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(lines()).toHaveLength(0);
    expect(store.get().bleFailed).toEqual({});
  });
});
