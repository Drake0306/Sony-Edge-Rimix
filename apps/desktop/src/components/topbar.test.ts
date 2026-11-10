import { describe, expect, test, vi } from 'vitest';
import cameraState from '../fixtures/camera-state.json';
import type { Actions } from '../actions';
import type { CameraState } from '../ipc';
import { createStore, initialState } from '../state';
import { mount } from './topbar';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => Promise.resolve()) }));

describe('top bar', () => {
  test('a lock shows while the camera is reached through Access Authentication', () => {
    const camera = { ...(cameraState as unknown as CameraState), connected: true, secured: false };
    const store = createStore({ ...initialState(), screen: 'main' as const, camera });
    const root = document.createElement('div');
    const unmount = mount(root, store, {} as Actions);
    const lock = () => root.querySelector<HTMLElement>('[aria-label="Connected through Access Authentication"]');
    expect(lock()!.hidden).toBe(true);
    store.set({ camera: { ...camera, secured: true } });
    expect(lock()!.hidden).toBe(false);
    expect(lock()!.querySelector('svg')).not.toBeNull();
    expect(lock()!.getAttribute('role')).toBe('img');
    unmount();
  });
});
