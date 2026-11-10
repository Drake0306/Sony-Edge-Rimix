import { describe, expect, test } from 'vitest';
import tagged from './fixtures/state-changed.json';
import taggedSaved from './fixtures/capture-saved.json';
import cardObject from './fixtures/card-object.json';
import { activate, applyEvent, cameraBusy, createStore, defaultSettings, emptyCard, initialState, type AppState } from './state';
import type { CameraEvent, CameraState, CardObject } from './ipc';
import { start } from './interval';

// The fixtures are tagged with the camera they came from.
const fixture = tagged.event as unknown;
const captureSaved = taggedSaved.event as unknown;

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
    expect(s.captures[0]).toEqual({
      path: '/p/DSC00001-1.JPG', filename: 'DSC00001-1.JPG', bytes: 3, cameraId: null, cameraLabel: '',
    });
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

  test('a plain link loss keeps a running interval, a final SSH failure ends it', () => {
    const camera = (fixture as unknown as { StateChanged: CameraState }).StateChanged;
    const s = { ...initialState(), screen: 'main' as const, camera, interval: start({ everyS: 5, count: null }, 0) };
    expect(applyEvent(s, { Disconnected: { reason: 'connection reset' } })).toEqual({});
    const final = applyEvent(s, { Disconnected: { reason: 'the camera refused the username or password' } });
    expect(final.interval).toBeNull();
  });

  test('events without state changes leave the state alone', () => {
    expect(applyEvent(initialState(), 'Connected')).toEqual({});
    expect(applyEvent(initialState(), { CaptureSkipped: { filename: 'DSC00001.JPG' } })).toEqual({});
    expect(applyEvent({ ...initialState(), reconnecting: 2 }, 'Connected')).toEqual({ reconnecting: null });
  });
});

describe('several cameras', () => {
  const base = (fixture as unknown as { StateChanged: CameraState }).StateChanged;
  const zv = { ...base, model: 'ZV-E10' };
  const a7 = { ...base, model: 'ILCE-7M4' };
  const A = '10.0.0.5:15740';
  const B = 'DIRECT-zz99:ILCE-7M4';
  const two = (): AppState => ({
    ...initialState(),
    screen: 'main',
    cameras: { [A]: zv, [B]: a7 },
    activeId: A,
    camera: zv,
  });
  const step = (s: AppState, event: CameraEvent, id: string) => ({ ...s, ...applyEvent(s, event, id) });

  test('a state change from another camera updates it without touching the active camera', () => {
    const s = two();
    const changed = { ...a7, battery: 10 };
    const next = step(s, { StateChanged: changed }, B);
    expect(next.cameras[B]).toBe(changed);
    expect(next.cameras[A]).toBe(zv);
    expect(next.camera).toBe(zv);
    expect(next.status).toBe(s.status);
    // From the active camera, both follow.
    const own = { ...zv, battery: 50 };
    const after = step(next, { StateChanged: own }, A);
    expect(after.camera).toBe(own);
    expect(after.cameras[A]).toBe(own);
  });

  test('events from a camera that is not connected are ignored', () => {
    expect(applyEvent(two(), { StateChanged: a7 }, 'gone:1')).toEqual({});
  });

  test('both cameras saving DSC00001.JPG keep both captures, each naming its camera', () => {
    let s = two();
    s = step(s, { CaptureProgress: { filename: 'DSC00001.JPG', received: 1, total: 3 } }, A);
    s = step(s, { CaptureProgress: { filename: 'DSC00001.JPG', received: 2, total: 3 } }, B);
    expect(Object.keys(s.transfers).sort()).toEqual([`${A}/DSC00001.JPG`, `${B}/DSC00001.JPG`].sort());
    const saved = (filename: string): CameraEvent => ({
      CaptureSaved: { path: `/p/${filename}`, bytes: 3, filename, camera_filename: 'DSC00001.JPG' },
    });
    s = step(s, saved('DSC00001.JPG'), A);
    expect(Object.keys(s.transfers)).toEqual([`${B}/DSC00001.JPG`]);
    s = step(s, saved('DSC00001-1.JPG'), B);
    expect(s.transfers).toEqual({});
    expect(s.captures.map((c) => [c.filename, c.cameraId, c.cameraLabel])).toEqual([
      ['DSC00001-1.JPG', B, 'ILCE-7M4'],
      ['DSC00001.JPG', A, 'ZV-E10'],
    ]);
  });

  test('reconnecting is tracked per camera and shown for the active one', () => {
    let s = step(two(), { Reconnecting: { attempt: 2 } }, B);
    expect(s.reconnecting).toBeNull();
    expect(s.attempts).toEqual({ [B]: 2 });
    expect(cameraBusy(s, B)).toBe(true);
    expect(cameraBusy(s, A)).toBe(false);
    s = step(s, 'Connected', B);
    expect(s.attempts).toEqual({});
    s = step(s, { Reconnecting: { attempt: 1 } }, A);
    expect(s.reconnecting).toBe(1);
  });

  test('a busy camera is judged by its own transfers', () => {
    const s = { ...two(), transfers: { [`${B}/DSC00002.JPG`]: { received: 1, total: 3 } } };
    expect(cameraBusy(s, B)).toBe(true);
    expect(cameraBusy(s, A)).toBe(false);
  });

  test('a final SSH failure on another camera ends only a run on that camera', () => {
    const refused: CameraEvent = { Disconnected: { reason: 'the camera refused the username or password' } };
    const onA = { ...two(), interval: start({ everyS: 5, count: null }, 0, A) };
    const patch = applyEvent(onA, refused, B);
    expect(patch.interval).toBeUndefined();
    expect(patch.cameras?.[B].connected).toBe(false);
    expect(patch.camera).toBeUndefined();
    expect(applyEvent(onA, refused, A).interval).toBeNull();
  });
});

test('default settings carry the connection and preset fields', () => {
  const s = defaultSettings('/x');
  expect(s.wifiJoin).toBe('Automatic');
  expect(s.webcamId).toBeNull();
  expect(s.presets).toEqual([]);
});

describe('card', () => {
  const base = (fixture as unknown as { StateChanged: CameraState }).StateChanged;
  const zv = { ...base, model: 'ZV-E10' };
  const a7 = { ...base, model: 'ILCE-7M4' };
  const A = '10.0.0.5:15740';
  const B = 'DIRECT-zz99:ILCE-7M4';
  const jpeg = cardObject as CardObject;
  const clip: CardObject = { ...jpeg, handle: 259, filename: 'C0001.MP4', kind: 'Video', bytes: 64 << 20 };
  const browsing = (): AppState => ({
    ...initialState(),
    screen: 'main',
    cameras: { [A]: zv, [B]: a7 },
    activeId: A,
    camera: zv,
    cardOpen: true,
    card: { ...emptyCard(), items: [jpeg, clip], downloads: { 259: { received: 0, total: 64 << 20 } } },
  });
  const step = (s: AppState, event: CameraEvent, id: string) => ({ ...s, ...applyEvent(s, event, id) });
  const progress = (received: number): CameraEvent => ({
    CardDownloadProgress: { handle: 259, filename: 'C0001.MP4', received, total: 64 << 20 },
  });

  test('progress from the active camera is kept by handle', () => {
    const s = step(browsing(), progress(1 << 20), A);
    expect(s.card.downloads).toEqual({ 259: { received: 1 << 20, total: 64 << 20 } });
    expect(s.card.items).toEqual([jpeg, clip]);
  });

  test('progress from another camera leaves the card alone', () => {
    expect(applyEvent(browsing(), progress(1 << 20), B)).toEqual({});
  });

  test('progress for a file not being downloaded is ignored', () => {
    const s = browsing();
    const other: CameraEvent = { CardDownloadProgress: { handle: 257, filename: 'DSC00010.JPG', received: 1, total: 2 } };
    expect(applyEvent(s, other, A)).toEqual({});
  });

  test('a progress event after the reply does not bring the row back', () => {
    // The reply came first: the row is saved and no longer downloading.
    const s: AppState = { ...browsing(), card: { ...browsing().card, downloads: {}, done: { 259: '/shots/C0001.MP4' } } };
    const next = step(s, progress(64 << 20), A);
    expect(next.card).toBe(s.card);
  });

  test('a save with the same name as a downloading file does not mark its row saved', () => {
    const s = browsing();
    const saved: CameraEvent = {
      CaptureSaved: { path: '/shots/C0001.MP4', bytes: 64 << 20, filename: 'C0001.MP4', camera_filename: 'C0001.MP4' },
    };
    const next = step(s, saved, A);
    expect(next.card).toBe(s.card);
    // It still lands in the filmstrip.
    expect(next.captures[0].path).toBe('/shots/C0001.MP4');
  });

  test('a failure with the same name as a downloading file leaves the row to the reply', () => {
    const s = browsing();
    const next = step(s, { CaptureFailed: { filename: 'C0001.MP4', error: 'timed out' } }, A);
    expect(next.card).toBe(s.card);
    expect(next.saveError).toEqual({ filename: 'C0001.MP4', error: 'timed out' });
  });

  test('switching the active camera clears the card and closes the panel', () => {
    const s = browsing();
    const patch = activate(s, B);
    expect(patch.card).toEqual(emptyCard());
    expect(patch.cardOpen).toBe(false);
    // Activating the camera already active keeps it.
    expect(activate(s, A).card).toBeUndefined();
  });
});
