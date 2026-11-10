// Presets: app-side named bundles of camera property values, stored in the
// App settings per camera model, in slots C1 to C4. Applying one writes each
// value through `Camera::set`, which refuses values outside the property's
// own settable list.

import type { ApplyOutcome, AppSettings, CameraState, Preset, PresetValue } from './ipc';
import { CHIP_CODES, findProp, isColourTemp, isUsable, PROP, SETTING_NAMES } from './props';

export const PRESET_SLOTS = 4;

const validSlot = (slot: number) => Number.isInteger(slot) && slot >= 1 && slot <= PRESET_SLOTS;

/** The preset in `slot` for camera `model`; slots outside 1 to 4 never match. */
export function presetFor(
  settings: AppSettings | null,
  model: string | undefined,
  slot: number,
): Preset | undefined {
  if (!settings || !model || !validSlot(slot)) return undefined;
  return settings.presets.find((p) => p.model === model && p.slot === slot);
}

/** The camera's current values worth saving: usable chip settings, plus colour temp when white balance uses it. */
export function captureValues(camera: CameraState): PresetValue[] {
  const codes: number[] = [...CHIP_CODES];
  if (isColourTemp(camera)) codes.push(PROP.colourTemp);
  const values: PresetValue[] = [];
  for (const code of codes) {
    const prop = findProp(camera, code);
    if (prop && isUsable(prop)) values.push({ code, raw: prop.raw });
  }
  return values;
}

const newId = (): string =>
  typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
    ? crypto.randomUUID()
    : String(Date.now());

export function newPreset(name: string, model: string, slot: number, values: PresetValue[]): Preset {
  return { id: newId(), name, model, slot, values };
}

/** Replaces the preset with the same model and slot, or the same id; otherwise adds it. */
export function upsertPreset(presets: Preset[], preset: Preset): Preset[] {
  const same = (p: Preset) => p.id === preset.id || (p.model === preset.model && p.slot === preset.slot);
  if (!presets.some(same)) return [...presets, preset];
  let placed = false;
  const out: Preset[] = [];
  for (const p of presets) {
    if (!same(p)) out.push(p);
    else if (!placed) {
      out.push(preset);
      placed = true;
    }
  }
  return out;
}

const hex = (code: number) => `0x${code.toString(16).toUpperCase().padStart(4, '0')}`;

/** `Could not set Aperture, Mode`, or null when every value was taken. */
export function describeFailures(outcomes: ApplyOutcome[]): string | null {
  const names = outcomes.filter((o) => o.error !== null).map((o) => SETTING_NAMES[o.code] ?? hex(o.code));
  return names.length ? `Could not set ${names.join(', ')}` : null;
}
