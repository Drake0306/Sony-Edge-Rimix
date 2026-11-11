import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { createActions } from '../actions';
import type { CameraState, FeedStatus } from '../ipc';
import { createStore, initialState, type AppState, type AppStore } from '../state';
import { mount } from './webcamMode';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const OFF: FeedStatus = { enabled: false, url: null, stream_url: null, quality: 'Full', width: null, height: null, viewers: 0 };
const ON: FeedStatus = {
  enabled: true,
  url: 'http://127.0.0.1:8717/',
  stream_url: 'http://127.0.0.1:8717/stream.mjpg',
  quality: 'Full',
  width: 1024,
  height: 576,
  viewers: 0,
};
const camera = { model: 'ZV-E10', properties: [] } as unknown as CameraState;

let root: HTMLElement;
let store: AppStore;
let unmount: () => void;
let status: FeedStatus;

function setup(state: Partial<AppState> = {}): void {
  document.body.innerHTML = '';
  root = document.createElement('section');
  document.body.append(root);
  store = createStore<AppState>({ ...initialState(), screen: 'main', camera, ...state });
  unmount = mount(root, store, createActions(store));
}
const panel = () => root.querySelector<HTMLElement>('.feedpanel')!;
const flush = () => vi.advanceTimersByTimeAsync(0);

describe('webcam mode', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    status = OFF;
    vi.mocked(invoke).mockReset().mockImplementation((cmd: string, args?: unknown) => {
      const a = (args ?? {}) as { enabled?: boolean; quality?: FeedStatus['quality'] };
      if (cmd === 'webcam_set') status = a.enabled ? ON : OFF;
      if (cmd === 'webcam_set_quality') status = { ...status, quality: a.quality! };
      return Promise.resolve(status);
    });
  });
  afterEach(() => {
    unmount();
    vi.useRealTimers();
  });

  test('off, there is no panel and the live view is left alone', async () => {
    setup();
    await flush();
    expect(invoke).toHaveBeenCalledWith('webcam_status');
    expect(panel().hidden).toBe(true);
    expect(root.classList.contains('webcam-on')).toBe(false);
  });

  test('on, the panel replaces the live view and says how other apps get the picture', async () => {
    status = ON;
    setup();
    await flush();
    expect(panel().hidden).toBe(false);
    expect(root.classList.contains('webcam-on')).toBe(true);
    expect(panel().querySelector('h2')!.textContent).toBe('ZV-E10 is in use as a webcam');
    expect(panel().querySelector('.addr code')!.textContent).toBe('http://127.0.0.1:8717/');
    expect(panel().textContent).toContain('Start Virtual Camera');
    expect(panel().textContent).toContain("this computer's microphone");
    expect(panel().querySelector('.watch')!.textContent).toBe('No app is showing the picture yet');
  });

  test('the quality choices show their sizes and choosing one tells the app', async () => {
    status = ON;
    setup();
    await flush();
    const choices = [...panel().querySelectorAll<HTMLButtonElement>('.quality button')];
    expect(choices.map((b) => b.textContent)).toEqual(['Full1024 × 576', 'Medium768 × 432', 'Low512 × 288']);
    expect(choices.map((b) => b.getAttribute('aria-checked'))).toEqual(['true', 'false', 'false']);
    choices[2].click();
    await flush();
    expect(invoke).toHaveBeenCalledWith('webcam_set_quality', { quality: 'Low' });
    expect([...panel().querySelectorAll('.quality button')].map((b) => b.getAttribute('aria-checked'))).toEqual([
      'false',
      'false',
      'true',
    ]);
  });

  test('the count of apps watching follows the feed while it is on', async () => {
    status = ON;
    setup();
    await flush();
    status = { ...ON, viewers: 1 };
    await vi.advanceTimersByTimeAsync(2000);
    expect(panel().querySelector('.watch')!.textContent).toBe('1 app is showing the picture');
    status = { ...ON, viewers: 2 };
    await vi.advanceTimersByTimeAsync(2000);
    expect(panel().querySelector('.watch')!.textContent).toBe('2 apps are showing the picture');
  });

  test('Stop turns it off and the live view comes back', async () => {
    status = ON;
    setup();
    await flush();
    panel().querySelector<HTMLButtonElement>('button.stop')!.click();
    await flush();
    expect(invoke).toHaveBeenCalledWith('webcam_set', { enabled: false });
    expect(panel().hidden).toBe(true);
    expect(root.classList.contains('webcam-on')).toBe(false);
    // And nothing polls while it is off.
    vi.mocked(invoke).mockClear();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(invoke).not.toHaveBeenCalled();
  });
});
