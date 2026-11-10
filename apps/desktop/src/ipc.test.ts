import { describe, expect, test } from 'vitest';
import tagged from './fixtures/state-changed.json';
import taggedSaved from './fixtures/capture-saved.json';
import {
  isCaptureSaved,
  isConnected,
  isDisconnected,
  isStateChanged,
  type CameraEvent,
  type TaggedEvent,
} from './ipc';

// The fixture is the exact JSON pinned by the Rust test
// `state_changed_event_json_is_stable` in src-tauri/src/state.rs: each event
// arrives tagged with the camera it came from.
const fixture = tagged.event as unknown;
const captureSaved = taggedSaved.event as unknown;

describe('camera events', () => {
  test('events are tagged with their camera id', () => {
    const t = tagged as unknown as TaggedEvent;
    expect(t.camera_id).toBe('192.168.122.1:15740');
    expect(isStateChanged(t.event)).toBe(true);
    expect((taggedSaved as unknown as TaggedEvent).camera_id).toBe('192.168.122.1:15740');
  });

  test('cameraEventSchema_parses_state_changed_payload', () => {
    const event = fixture as unknown as CameraEvent;
    expect(isStateChanged(event)).toBe(true);
    if (!isStateChanged(event)) return;
    const state = event.StateChanged;
    expect(state.model).toBe('ZV-E10');
    expect(state.secured).toBe(true);
    expect(state.battery).toBe(72);
    expect(state.focus_found).toBe(false);
    expect(state.live_view_fps).toBe(29.5);
    expect(state.last_shot_at).toBeNull();
    const prop = state.properties[0];
    expect(prop.code).toBe(0x500d);
    expect(prop.raw).toEqual({ U32: 65661 });
    expect(prop.enabled).toBe('On');
    expect(prop.choices[0].label).toBe('1/125');
    expect(isConnected(event)).toBe(false);
  });

  // Pinned by the Rust test `capture_saved_event_json_is_stable`.
  test('a saved shot names both the saved file and the camera file', () => {
    const event = captureSaved as unknown as CameraEvent;
    expect(isCaptureSaved(event)).toBe(true);
    if (!isCaptureSaved(event)) return;
    expect(event.CaptureSaved.filename).toBe('DSC00001-1.JPG');
    expect(event.CaptureSaved.camera_filename).toBe('DSC00001.JPG');
    expect(event.CaptureSaved.path).toBe('/p/DSC00001-1.JPG');
    expect(event.CaptureSaved.bytes).toBe(3);
  });

  test('unit variants arrive as bare strings', () => {
    const event = JSON.parse('"Connected"') as CameraEvent;
    expect(isConnected(event)).toBe(true);
    expect(isStateChanged(event)).toBe(false);
    expect(isDisconnected(event)).toBe(false);
  });

  test('struct variants are tagged objects', () => {
    const gone = JSON.parse('{"Disconnected":{"reason":"timeout"}}') as CameraEvent;
    expect(isDisconnected(gone)).toBe(true);
    if (isDisconnected(gone)) expect(gone.Disconnected.reason).toBe('timeout');
    const saved = JSON.parse(
      '{"CaptureSaved":{"path":"/p/DSC00001.JPG","bytes":3,"filename":"DSC00001.JPG","camera_filename":"DSC00001.JPG"}}',
    ) as CameraEvent;
    expect(isCaptureSaved(saved)).toBe(true);
    expect(isStateChanged(saved)).toBe(false);
  });
});
