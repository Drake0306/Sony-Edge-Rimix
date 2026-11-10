import { describe, expect, it } from 'vitest';
import cameraState from './fixtures/camera-state.json';
import type { CameraState, Preset } from './ipc';
import { PROP } from './props';
import { defaultSettings } from './state';
import { captureValues, describeFailures, newPreset, presetFor, PRESET_SLOTS, upsertPreset } from './presets';

const camera = cameraState as unknown as CameraState;

const talking: Preset = { id: 'a', name: 'Talking head', model: 'ZV-E10', slot: 1, values: [] };
const other: Preset = { id: 'b', name: 'Night', model: 'ILCE-7M4', slot: 2, values: [] };
const settings = { ...defaultSettings('/tmp'), presets: [talking, other] };

describe('presets', () => {
  it('has four slots', () => expect(PRESET_SLOTS).toBe(4));

  it('captures only usable chip values and the colour temperature when active', () => {
    const values = captureValues(camera);
    expect(values.map((v) => v.code)).toEqual([
      PROP.mode, PROP.shutter, PROP.aperture, PROP.iso, PROP.exposureComp,
      PROP.whiteBalance, PROP.focus, PROP.drive, PROP.format, PROP.colourTemp,
    ]);
    expect(values.find((v) => v.code === PROP.iso)).toEqual({ code: PROP.iso, raw: { U32: 800 } });

    // A disabled or read-only prop is left out; so is colour temp once WB is not colour temp.
    const props = camera.properties.map((p) => {
      if (p.code === PROP.iso) return { ...p, enabled: 'Off' as const };
      if (p.code === PROP.drive) return { ...p, writable: false };
      if (p.code === PROP.whiteBalance) return { ...p, raw: { U16: 2 } };
      return p;
    });
    const codes = captureValues({ ...camera, properties: props }).map((v) => v.code);
    expect(codes).not.toContain(PROP.iso);
    expect(codes).not.toContain(PROP.drive);
    expect(codes).not.toContain(PROP.colourTemp);
    expect(codes).toContain(PROP.whiteBalance);
  });

  it('finds a preset by model and slot only', () => {
    expect(presetFor(settings, 'ILCE-7M4', 1)).toBeUndefined();
    expect(presetFor(settings, 'ZV-E10', 1)?.name).toBe('Talking head');
    expect(presetFor(settings, 'ZV-E10', 2)).toBeUndefined();
    expect(presetFor(null, 'ZV-E10', 1)).toBeUndefined();
    expect(presetFor(settings, undefined, 1)).toBeUndefined();
    // Slots outside 1 to 4 are ignored.
    const odd = { ...settings, presets: [{ ...talking, slot: 5 }] };
    expect(presetFor(odd, 'ZV-E10', 5)).toBeUndefined();
  });

  it('makes a new preset with an id', () => {
    const values = [{ code: PROP.iso, raw: { U32: 800 } }];
    const a = newPreset('Day', 'ZV-E10', 3, values);
    expect(a).toMatchObject({ name: 'Day', model: 'ZV-E10', slot: 3, values });
    expect(a.id).toBeTruthy();
    expect(newPreset('Day', 'ZV-E10', 3, values).id).not.toBe(a.id);
  });

  it('upsert replaces the same slot for the same model', () => {
    const replacement = { ...talking, id: 'c', name: 'Vlog' };
    expect(upsertPreset([talking, other], replacement)).toEqual([replacement, other]);
    // Same slot, other model: added.
    const third = { ...other, id: 'd', slot: 1 };
    expect(upsertPreset([talking, other], third)).toEqual([talking, other, third]);
    // Same id: replaced (a rename).
    const renamed = { ...other, name: 'Late' };
    expect(upsertPreset([talking, other], renamed)).toEqual([talking, renamed]);
  });

  it('describes failures by setting name', () => {
    expect(
      describeFailures([{ code: 0x5007, error: 'nope' }, { code: 0xd21e, error: null }, { code: 0x500e, error: 'x' }]),
    ).toBe('Could not set Aperture, Mode');
    expect(describeFailures([{ code: 0x5007, error: null }])).toBeNull();
    expect(describeFailures([])).toBeNull();
    expect(describeFailures([{ code: 0xd2ab, error: 'x' }])).toBe('Could not set 0xD2AB');
  });
});
