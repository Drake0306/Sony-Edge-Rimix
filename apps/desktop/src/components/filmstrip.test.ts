import { describe, expect, test, vi } from 'vitest';
import cameraState from '../fixtures/camera-state.json';
import type { Actions } from '../actions';
import type { CameraState } from '../ipc';
import { createStore, initialState, type AppState, type CaptureItem } from '../state';
import { mount } from './filmstrip';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => Promise.reject(new Error('no thumbnails'))) }));

const base = cameraState as unknown as CameraState;
const A = '10.0.0.5:15740';
const B = 'DIRECT-zz99:ILCE-7M4';
const shot = (filename: string, cameraId: string, cameraLabel: string): CaptureItem => ({
  path: `/p/${filename}`, filename, bytes: 3, cameraId, cameraLabel,
});

function mountWith(state: Partial<AppState>) {
  const store = createStore<AppState>({ ...initialState(), screen: 'main', ...state });
  const root = document.createElement('div');
  const unmount = mount(root, store, {} as Actions);
  const labels = () => [...root.querySelectorAll('.thumb .cam')].map((c) => c.textContent);
  return { root, store, unmount, labels };
}

describe('filmstrip', () => {
  test('is shown only once copying after each shot is asked for', () => {
    const { root, store, unmount } = mountWith({});
    const film = root.querySelector<HTMLElement>('.film')!;
    expect(film.hidden).toBe(true);
    store.set({ settings: { copyFeature: true } as AppState['settings'] });
    expect(film.hidden).toBe(false);
    unmount();
  });

  test('with two cameras each tile names its camera, transfers included', () => {
    const { root, unmount, labels } = mountWith({
      cameras: { [A]: { ...base, model: 'ZV-E10' }, [B]: { ...base, model: 'ILCE-7M4' } },
      activeId: A,
      captures: [shot('DSC00001-1.ARW', B, 'ILCE-7M4'), shot('DSC00001.ARW', A, 'ZV-E10')],
      transfers: { [`${B}/DSC00002.ARW`]: { received: 1, total: 2 } },
    });
    expect(labels()).toEqual(['ILCE-7M4', 'ILCE-7M4', 'ZV-E10']);
    const names = [...root.querySelectorAll('.thumb > .n')].map((n) => n.firstChild?.textContent);
    expect(names).toEqual(['DSC00002', 'DSC00001-1', 'DSC00001']);
    unmount();
  });

  test('with one camera there is no camera label', () => {
    const { store, unmount, labels } = mountWith({
      cameras: { [A]: { ...base, model: 'ZV-E10' } },
      activeId: A,
      captures: [shot('DSC00001.ARW', A, 'ZV-E10')],
    });
    expect(labels()).toEqual([]);
    store.set({ cameras: { [A]: { ...base, model: 'ZV-E10' }, [B]: { ...base, model: 'ILCE-7M4' } } });
    expect(labels()).toEqual(['ZV-E10']);
    unmount();
  });

  test('files received over SFTP are labelled Received even with one camera or none', () => {
    const { store, unmount, labels } = mountWith({
      cameras: { [A]: { ...base, model: 'ZV-E10' } },
      activeId: A,
      captures: [shot('DSC00009.JPG', 'sftp', 'Received'), shot('DSC00001.ARW', A, 'ZV-E10')],
    });
    expect(labels()).toEqual(['Received']);
    store.set({ cameras: {}, activeId: null });
    expect(labels()).toEqual(['Received']);
    unmount();
  });

  test('files arriving are summed up in one progress bar at the top', () => {
    const { root, store, unmount } = mountWith({
      transfers: {
        [`${A}/DSC00412.ARW`]: { received: 10_000_000, total: 25_000_000 },
        [`${A}/C0007.MP4`]: { received: 20_000_000, total: 75_000_000 },
      },
    });
    const total = root.querySelector<HTMLElement>('.film .total')!;
    expect(total.hidden).toBe(false);
    expect(total.querySelector('.what')!.textContent).toBe('Copying 2 files to this computer');
    expect(total.querySelector('.pct')!.textContent).toBe('30%');
    const bar = total.querySelector<HTMLElement>('[role="progressbar"]')!;
    expect(bar.getAttribute('aria-valuenow')).toBe('30');
    expect(bar.querySelector<HTMLElement>('i')!.style.width).toBe('30%');
    // One file, named; a size not known yet counts as nothing so far.
    store.set({ transfers: { [`${A}/C0007.MP4`]: { received: 0, total: 0 } } });
    expect(total.querySelector('.what')!.textContent).toBe('Copying C0007.MP4 to this computer');
    expect(total.querySelector('.pct')!.textContent).toBe('0%');
    store.set({ transfers: {} });
    expect(total.hidden).toBe(true);
    unmount();
  });
});
