import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import type { AppSettings } from '../ipc';
import { createStore, initialState } from '../state';
import { createActions } from '../actions';
import { mount, shortcutHint } from './appSettings';
import { listWebcams } from '../qr/scanner';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => Promise.resolve()) }));
vi.mock('../qr/scanner', () => ({
  listWebcams: vi.fn(() =>
    Promise.resolve([
      { id: 'cam-a', label: 'Desk cam' },
      { id: 'cam-b', label: 'Laptop cam' },
    ]),
  ),
  webcamSupported: () => true,
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

const settings: AppSettings = {
  appearance: 'Dark',
  layout: 'Dock',
  destinationMode: 'FolderPerDate',
  destinationBase: '/Users/roy/Pictures/Rimix',
  recentCameras: [{ ip: '192.168.122.1', model: 'ZV-E10', lastUsed: 1700000000000, ssid: null }],
  lastIp: '192.168.122.1',
  wifiJoin: 'Automatic',
  webcamId: null,
  presets: [],
};

const mounted: (() => void)[] = [];
afterEach(() => mounted.splice(0).forEach((u) => u()));

function setup(over: Partial<AppSettings> = {}) {
  document.body.innerHTML = '';
  const root = document.createElement('div');
  document.body.append(root);
  const store = createStore({ ...initialState(), screen: 'main' as const, settings: { ...settings, ...over } });
  const actions = createActions(store);
  const forget = vi.spyOn(actions, 'forgetCamera').mockResolvedValue();
  const unmount = mount(root, store, actions);
  mounted.push(unmount);
  return { root, store, unmount, forget };
}

const radios = (root: HTMLElement, label: string) =>
  [...root.querySelectorAll<HTMLButtonElement>(`.apppop [aria-label="${label}"] button`)];
const press = (key: string, init: KeyboardEventInit = {}) =>
  document.dispatchEvent(new KeyboardEvent('keydown', { key, cancelable: true, ...init }));

describe('app settings', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    vi.mocked(listWebcams).mockClear();
    document.documentElement.setAttribute('data-theme', 'dark');
    document.documentElement.setAttribute('data-layout', 'dock');
  });

  test('settings_roundtrip_through_ipc_mock', () => {
    const { root, store } = setup();
    const theme = radios(root, 'Theme');
    expect(theme.map((b) => b.textContent)).toEqual(['Dark', 'Light', 'System']);
    expect(theme[0].getAttribute('aria-checked')).toBe('true');
    theme[1].click();
    expect(invoke).toHaveBeenCalledWith('save_settings', { settings: { ...settings, appearance: 'Light' } });
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
    expect(store.get().settings?.appearance).toBe('Light');
    expect(radios(root, 'Theme')[1].getAttribute('aria-checked')).toBe('true');
  });

  test('layout row saves and applies the layout, keeping the theme just chosen', () => {
    const { root } = setup();
    radios(root, 'Theme')[1].click();
    const layout = radios(root, 'Controls layout');
    expect(layout.map((b) => b.textContent)).toEqual(['Dock', 'Ring']);
    layout[1].click();
    expect(invoke).toHaveBeenLastCalledWith('save_settings', {
      settings: { ...settings, appearance: 'Light', layout: 'Ring' },
    });
    expect(document.documentElement.getAttribute('data-layout')).toBe('ring');
  });

  test('popover rows, Save to and the shortcut hint', () => {
    const { root } = setup();
    const heads = [...root.querySelectorAll('.apppop .sect .h span')].map((s) => s.textContent);
    expect(heads).toEqual(['Appearance', 'Layout', 'Connection', 'Save to']);
    expect(root.querySelector('.apppop .hint')!.textContent).toMatch(/^Shortcut: (⌘|Ctrl\+), opens settings$/);
  });

  test('shortcut hint names the right key per platform', () => {
    expect(shortcutHint('MacIntel')).toBe('Shortcut: ⌘, opens settings');
    expect(shortcutHint('Win32')).toBe('Shortcut: Ctrl+, opens settings');
  });

  test('Cmd or Ctrl plus comma toggles the popover and Escape closes it', () => {
    const { root } = setup();
    const pop = root.querySelector<HTMLElement>('.apppop')!;
    const gear = root.querySelector<HTMLElement>('.gear')!;
    expect(pop.hidden).toBe(true);
    press(',', { metaKey: true });
    expect(pop.hidden).toBe(false);
    expect(gear.getAttribute('aria-expanded')).toBe('true');
    press(',', { ctrlKey: true });
    expect(pop.hidden).toBe(true);
    gear.click();
    expect(pop.hidden).toBe(false);
    press('Escape');
    expect(pop.hidden).toBe(true);
    press(',');
    expect(pop.hidden).toBe(true);
  });

  test('unmount removes the gear and stops the shortcut', () => {
    const { root, unmount } = setup();
    unmount();
    expect(root.querySelector('.gear, .apppop')).toBeNull();
    press(',', { metaKey: true });
  });

  test('a Logs row under Connection opens the log folder', () => {
    const { root } = setup();
    const connection = [...root.querySelectorAll('.apppop .sect')].find(
      (s) => s.querySelector('.h span')?.textContent === 'Connection',
    )!;
    const row = [...connection.querySelectorAll('.r')].find((r) => r.querySelector('.k')?.textContent === 'Logs')!;
    const show = row.querySelector<HTMLButtonElement>('button')!;
    expect(show.textContent).toBe('Show log folder');
    show.click();
    expect(invoke).toHaveBeenLastCalledWith('open_logs');
  });

  test('offers Automatic and Ask me for Wi-Fi join and saves the choice', () => {
    const { root } = setup();
    const join = radios(root, 'Wi-Fi join');
    expect(join.map((b) => b.textContent)).toEqual(['Automatic', 'Ask me']);
    expect(join[0].getAttribute('aria-checked')).toBe('true');
    join[1].click();
    expect(invoke).toHaveBeenLastCalledWith('save_settings', { settings: { ...settings, wifiJoin: 'Ask' } });
    expect(radios(root, 'Wi-Fi join')[1].getAttribute('aria-checked')).toBe('true');
  });

  test('lists webcams with Default first and saves the chosen one', async () => {
    const { root } = setup();
    press(',', { metaKey: true });
    await vi.waitFor(() => expect(root.querySelectorAll('select[aria-label="Webcam"] option').length).toBe(3));
    const select = root.querySelector<HTMLSelectElement>('select[aria-label="Webcam"]')!;
    expect([...select.options].map((o) => o.textContent)).toEqual(['Default', 'Desk cam', 'Laptop cam']);
    expect(select.value).toBe('');
    select.value = 'cam-a';
    select.dispatchEvent(new Event('change'));
    expect(invoke).toHaveBeenLastCalledWith('save_settings', { settings: { ...settings, webcamId: 'cam-a' } });
    select.value = '';
    select.dispatchEvent(new Event('change'));
    expect(invoke).toHaveBeenLastCalledWith('save_settings', { settings: { ...settings, webcamId: null } });
  });

  test('a saved webcam that is gone shows Default without changing the setting', async () => {
    const { root, store } = setup({ webcamId: 'gone' });
    press(',', { metaKey: true });
    await vi.waitFor(() => expect(root.querySelectorAll('select option').length).toBe(3));
    expect(root.querySelector<HTMLSelectElement>('select')!.value).toBe('');
    expect(store.get().settings?.webcamId).toBe('gone');
  });

  test('only Default is offered when no webcam API exists', async () => {
    vi.mocked(listWebcams).mockResolvedValueOnce([]);
    const { root } = setup();
    press(',', { metaKey: true });
    await vi.waitFor(() => expect(listWebcams).toHaveBeenCalled());
    await new Promise((r) => setTimeout(r, 0));
    expect(root.querySelectorAll('select option').length).toBe(1);
  });

  test('lists remembered cameras with a Forget button', () => {
    const a = { ip: '192.168.122.1', model: 'ZV-E10', lastUsed: 1, ssid: 'DIRECT-ZV' };
    const b = { ip: '10.0.0.5', model: 'A7C', lastUsed: 2, ssid: null };
    const { root, forget, store } = setup({ recentCameras: [a, b] });
    const rows = root.querySelectorAll('.apppop .remembered .r');
    expect(rows.length).toBe(2);
    expect(rows[0].textContent).toContain('ZV-E10');
    expect(rows[0].textContent).toContain('DIRECT-ZV');
    expect(rows[1].textContent).toContain('10.0.0.5');
    root.querySelector<HTMLButtonElement>('[aria-label="Forget ZV-E10 on DIRECT-ZV"]')!.click();
    expect(forget).toHaveBeenCalledWith(a);
    expect(root.querySelector('[aria-label="Forget A7C on 10.0.0.5"]')).not.toBeNull();
    store.set({ settings: { ...settings, recentCameras: [b] } });
    expect(root.querySelectorAll('.apppop .remembered .r').length).toBe(1);
  });

  test('says None yet when nothing is remembered', () => {
    const { root } = setup({ recentCameras: [] });
    expect(root.querySelector('.apppop .remembered .quiet')!.textContent).toBe('None yet');
    expect(root.querySelector('.apppop .remembered .r')).toBeNull();
  });
});
