import { describe, expect, test, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import cameraState from './fixtures/camera-state.json';
import { eventToast, startApp } from './app';

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
