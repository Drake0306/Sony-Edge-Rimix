import { beforeEach, describe, expect, test, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import type { AppSettings } from '../ipc';
import { createStore, initialState, type AppStore } from '../state';
import { createActions } from '../actions';
import { mount, shortcutHint } from './appSettings';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => Promise.resolve()) }));
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

function setup(): { root: HTMLElement; store: AppStore; unmount: () => void } {
  document.body.innerHTML = '';
  const root = document.createElement('div');
  document.body.append(root);
  const store = createStore({ ...initialState(), screen: 'main' as const, settings });
  const unmount = mount(root, store, createActions(store));
  return { root, store, unmount };
}

const radios = (root: HTMLElement, label: string) =>
  [...root.querySelectorAll<HTMLButtonElement>(`.apppop [aria-label="${label}"] button`)];
const press = (key: string, init: KeyboardEventInit = {}) =>
  document.dispatchEvent(new KeyboardEvent('keydown', { key, cancelable: true, ...init }));

describe('app settings', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
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
    expect(heads).toEqual(['Appearance', 'Layout', 'Save to']);
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
});
