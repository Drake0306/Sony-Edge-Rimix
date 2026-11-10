import { describe, expect, test } from 'vitest';
import fixture from './fixtures/state-changed.json';
import {
  isCaptureSaved,
  isConnected,
  isDisconnected,
  isStateChanged,
  type CameraEvent,
} from './ipc';

// The fixture is the exact JSON pinned by the Rust test
// `state_changed_event_json_is_stable` in src-tauri/src/state.rs.
describe('camera events', () => {
  test('cameraEventSchema_parses_state_changed_payload', () => {
    const event = fixture as unknown as CameraEvent;
    expect(isStateChanged(event)).toBe(true);
    if (!isStateChanged(event)) return;
    const state = event.StateChanged;
    expect(state.model).toBe('ZV-E10');
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
      '{"CaptureSaved":{"path":"/p/DSC00001.JPG","bytes":3,"filename":"DSC00001.JPG"}}',
    ) as CameraEvent;
    expect(isCaptureSaved(saved)).toBe(true);
    expect(isStateChanged(saved)).toBe(false);
  });
});
