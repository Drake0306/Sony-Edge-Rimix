import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import type { Actions } from '../actions';
import { NOT_A_WIFI_CODE } from '../copy';
import type { AppSettings, RecentCamera } from '../ipc';
import { createStore, defaultSettings, initialState, type AppStore } from '../state';
import { mount } from './connectScreen';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => Promise.resolve([])) }));
vi.mock('../qr/scanner', () => ({
  webcamSupported: () => false,
  createScanner: vi.fn(() => ({ start: vi.fn(), stop: vi.fn() })),
}));

function fakeActions(): Actions {
  return {
    connect: vi.fn(() => Promise.resolve(true)),
    connectWithWifi: vi.fn(() => Promise.resolve(true)),
    connectRecent: vi.fn(() => Promise.resolve(true)),
    forgetCamera: vi.fn(() => Promise.resolve()),
  } as unknown as Actions;
}

let root: HTMLElement;
let store: AppStore;
let actions: Actions;
let unmount: () => void;

function setup(settings: Partial<AppSettings> = {}) {
  document.body.innerHTML = '';
  root = document.createElement('section');
  document.body.append(root);
  store = createStore({ ...initialState(), settings: { ...defaultSettings('/tmp'), ...settings } });
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
    const adv = root.querySelector<HTMLButtonElement>('button[aria-expanded]')!;
    expect(adv.textContent).toBe('Advanced: connect by address');
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
    expect(actions.connectWithWifi).toHaveBeenCalledWith({ ssid: 'DIRECT-abc1:ZV-E10', password: '12345678' });
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
    expect(actions.connect).toHaveBeenCalledWith('192.168.122.1');
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
