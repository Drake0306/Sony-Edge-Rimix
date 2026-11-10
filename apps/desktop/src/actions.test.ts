import { beforeEach, describe, expect, test, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import cameraState from './fixtures/camera-state.json';
import { createActions } from './actions';
import type { CameraState } from './ipc';
import { createStore, initialState } from './state';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const camera = cameraState as unknown as CameraState;
const REFUSED = 'Cannot connect to 10.0.0.9:15740: I/O error: Connection refused (os error 61)';

function answer(stateAfter: CameraState | null) {
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === 'connect') return Promise.reject(REFUSED);
    if (cmd === 'get_state') return Promise.resolve(stateAfter);
    return Promise.resolve();
  });
}

describe('connect from the main screen', () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  test('a failed switch that lost the old camera returns to the connect screen', async () => {
    answer(null);
    const store = createStore({ ...initialState(), screen: 'main' as const, camera });
    expect(await createActions(store).connect('10.0.0.9')).toBe(false);
    expect(store.get().screen).toBe('connect');
    expect(store.get().camera).toBeNull();
    expect(store.get().error).toContain('Could not reach the camera at 10.0.0.9.');
  });

  test('a failed connect keeps a camera that is still connected', async () => {
    answer(camera);
    const store = createStore({ ...initialState(), screen: 'main' as const, camera });
    expect(await createActions(store).connect('10.0.0.9')).toBe(false);
    expect(store.get().screen).toBe('main');
    expect(store.get().camera).toBe(camera);
  });
});
