import { describe, expect, test } from 'vitest';
import fixture from './fixtures/state-changed.json';
import captureSaved from './fixtures/capture-saved.json';
import { applyEvent, createStore, defaultSettings, initialState } from './state';
import type { CameraEvent, CameraState } from './ipc';

describe('store', () => {
  test('set merges and notifies subscribers', () => {
    const store = createStore({ a: 1, b: 'x' });
    const seen: number[] = [];
    const off = store.subscribe((s) => seen.push(s.a));
    store.set({ a: 2 });
    expect(store.get()).toEqual({ a: 2, b: 'x' });
    expect(seen).toEqual([2]);
    off();
    store.set({ a: 3 });
    expect(seen).toEqual([2]);
  });

  test('state_reducer_applies_state_changed', () => {
    const event = fixture as unknown as CameraEvent;
    const next = { ...initialState(), ...applyEvent(initialState(), event) };
    expect(next.screen).toBe('main');
    expect(next.camera?.model).toBe('ZV-E10');
    expect(next.camera?.properties[0].short).toBe('1/125');
    expect(next.status.fps).toBe(29.5);
    expect(next.status.lastShotAt).toBeNull();
  });

  test('captures keep the newest six and progress is keyed by filename', () => {
    let s = initialState();
    for (let i = 1; i <= 7; i++) {
      const name = `DSC0000${i}.JPG`;
      s = { ...s, ...applyEvent(s, { CaptureProgress: { filename: name, received: 5, total: 10 } }) };
      expect(s.transfers[name]).toEqual({ received: 5, total: 10 });
      s = {
        ...s,
        ...applyEvent(s, { CaptureSaved: { path: `/p/${name}`, bytes: 10, filename: name, camera_filename: name } }),
      };
      expect(s.transfers[name]).toBeUndefined();
    }
    expect(s.captures.map((c) => c.filename)).toEqual([
      'DSC00007.JPG', 'DSC00006.JPG', 'DSC00005.JPG', 'DSC00004.JPG', 'DSC00003.JPG', 'DSC00002.JPG',
    ]);
  });

  test('a save under a suffixed name clears the progress keyed by the camera name', () => {
    let s = initialState();
    s = { ...s, ...applyEvent(s, { CaptureProgress: { filename: 'DSC00001.JPG', received: 1, total: 3 } }) };
    s = { ...s, ...applyEvent(s, captureSaved as unknown as CameraEvent) };
    expect(s.transfers).toEqual({});
    expect(s.captures[0]).toEqual({ path: '/p/DSC00001-1.JPG', filename: 'DSC00001-1.JPG', bytes: 3 });
  });

  test('a failed save is kept as one persistent error until the next save', () => {
    let s = initialState();
    const failed: CameraEvent = {
      CaptureFailed: { filename: 'DSC00001.JPG', error: 'Cannot save to /x: Permission denied' },
    };
    s = { ...s, ...applyEvent(s, { CaptureProgress: { filename: 'DSC00001.JPG', received: 0, total: 3 } }) };
    s = { ...s, ...applyEvent(s, failed) };
    expect(s.saveError).toEqual({ filename: 'DSC00001.JPG', error: 'Cannot save to /x: Permission denied' });
    expect(s.transfers).toEqual({});
    // The same failure again changes nothing, so nothing re-renders.
    expect(applyEvent(s, failed)).toEqual({});
    s = { ...s, ...applyEvent(s, captureSaved as unknown as CameraEvent) };
    expect(s.saveError).toBeNull();
  });

  test('a refused SSH login ends reconnecting with its message and keeps the main screen', () => {
    const camera = (fixture as unknown as { StateChanged: CameraState }).StateChanged;
    const s = { ...initialState(), screen: 'main' as const, camera, reconnecting: 2 };
    const patch = applyEvent(s, { Disconnected: { reason: 'the camera refused the username or password' } });
    expect(patch.reconnecting).toBeNull();
    expect(patch.error).toBe(
      'The camera refused the username or password. Check Access Authen. Info on the camera and try again.',
    );
    expect(patch.camera?.connected).toBe(false);
    expect(patch.screen).toBeUndefined();
    const changed = applyEvent(s, {
      Disconnected: { reason: "the camera's fingerprint changed (expected SHA256:a, got SHA256:b)" },
    });
    expect(changed.error).toMatch(/^The camera's fingerprint changed\./);
    // A plain link loss changes nothing here: reconnecting reports itself.
    expect(applyEvent(s, { Disconnected: { reason: 'connection reset' } })).toEqual({});
  });

  test('events without state changes leave the state alone', () => {
    expect(applyEvent(initialState(), 'Connected')).toEqual({});
    expect(applyEvent(initialState(), { CaptureSkipped: { filename: 'DSC00001.JPG' } })).toEqual({});
    expect(applyEvent({ ...initialState(), reconnecting: 2 }, 'Connected')).toEqual({ reconnecting: null });
  });
});

test('default settings carry the connection and preset fields', () => {
  const s = defaultSettings('/x');
  expect(s.wifiJoin).toBe('Automatic');
  expect(s.webcamId).toBeNull();
  expect(s.presets).toEqual([]);
});
