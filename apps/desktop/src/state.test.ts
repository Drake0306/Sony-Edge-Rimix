import { describe, expect, test } from 'vitest';
import fixture from './fixtures/state-changed.json';
import { applyEvent, createStore, initialState } from './state';
import type { CameraEvent } from './ipc';

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
      s = { ...s, ...applyEvent(s, { CaptureSaved: { path: `/p/${name}`, bytes: 10, filename: name } }) };
      expect(s.transfers[name]).toBeUndefined();
    }
    expect(s.captures.map((c) => c.filename)).toEqual([
      'DSC00007.JPG', 'DSC00006.JPG', 'DSC00005.JPG', 'DSC00004.JPG', 'DSC00003.JPG', 'DSC00002.JPG',
    ]);
  });

  test('events without state changes leave the state alone', () => {
    expect(applyEvent(initialState(), 'Connected')).toEqual({});
    expect(applyEvent(initialState(), { CaptureSkipped: { filename: 'DSC00001.JPG' } })).toEqual({});
    expect(applyEvent({ ...initialState(), reconnecting: 2 }, 'Connected')).toEqual({ reconnecting: null });
  });
});
