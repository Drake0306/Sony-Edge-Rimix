// Which camera properties the dock and the settings popover show, and how.

import type { CameraState, PropertySummary, PtpValue } from './ipc';

export const PROP = {
  mode: 0x500e,
  shutter: 0xd20d,
  aperture: 0x5007,
  iso: 0xd21e,
  exposureComp: 0x5010,
  whiteBalance: 0x5005,
  focus: 0x500a,
  drive: 0x5013,
  format: 0x5004,
  colourTemp: 0xd20f,
} as const;

/** The nine chips in dock order; a tenth, Picture profile, has no code yet. */
export const CHIP_CODES = [
  PROP.mode, PROP.shutter, PROP.aperture, PROP.iso, PROP.exposureComp,
  PROP.whiteBalance, PROP.focus, PROP.drive, PROP.format,
];

export const SETTING_NAMES: Record<number, string> = {
  [PROP.mode]: 'Mode',
  [PROP.shutter]: 'Shutter',
  [PROP.aperture]: 'Aperture',
  [PROP.iso]: 'ISO',
  [PROP.exposureComp]: 'Exposure comp',
  [PROP.whiteBalance]: 'White balance',
  [PROP.focus]: 'Focus',
  [PROP.drive]: 'Drive',
  [PROP.format]: 'Format',
  [PROP.colourTemp]: 'Colour temp',
};

/** White balance raw values meaning "use the colour temperature" (as the Rust side's `WB_COLOUR_TEMP`). */
const WB_COLOUR_TEMP = [0x8012, 0x8007];
/** The ISO raw value meaning Auto; larger values carry noise reduction flags. */
const ISO_AUTO = 0xffffff;

export const findProp = (camera: CameraState | null, code: number): PropertySummary | undefined =>
  camera?.properties.find((p) => p.code === code);

export const sameRaw = (a: PtpValue, b: PtpValue): boolean => JSON.stringify(a) === JSON.stringify(b);

export function isColourTemp(camera: CameraState | null): boolean {
  const wb = findProp(camera, PROP.whiteBalance);
  return !!wb && 'U16' in wb.raw && WB_COLOUR_TEMP.includes(wb.raw.U16) && !!findProp(camera, PROP.colourTemp);
}

/** A property's `short`, with K added to a bare colour temperature. */
export function shortText(p: PropertySummary | undefined): string {
  if (!p) return '—';
  return p.code === PROP.colourTemp && /^\d+$/.test(p.short) ? `${p.short}K` : p.short;
}

/** The value a chip shows: `short`, or the kelvin value when WB is colour temp. */
export function chipValue(camera: CameraState | null, code: number): string {
  if (code === PROP.whiteBalance && isColourTemp(camera)) {
    return shortText(findProp(camera, PROP.colourTemp));
  }
  return findProp(camera, code)?.short ?? '—';
}

export const isUsable = (p: PropertySummary | undefined): boolean =>
  !!p && p.writable && p.enabled === 'On';

const u32Of = (raw: PtpValue): number | null => ('U32' in raw ? raw.U32 : null);

/**
 * The ISO values the camera can be set to, lowest first: its own stops
 * (100, 125, 160, ...), without Auto and the noise reduction variants.
 */
export function isoStops(iso: PropertySummary | undefined): number[] {
  const stops = (iso?.choices ?? [])
    .map((c) => u32Of(c.raw))
    .filter((n): n is number => n !== null && n < ISO_AUTO);
  return [...new Set(stops)].sort((a, b) => a - b);
}

/**
 * The stop nearest a typed ISO (halfway goes up), since a camera takes only
 * its own stops; null when there are none or `typed` is not a positive number.
 */
export function nearestIso(iso: PropertySummary | undefined, typed: number): { value: number; raw: PtpValue } | null {
  const stops = isoStops(iso);
  if (stops.length === 0 || !Number.isFinite(typed) || typed <= 0) return null;
  const value = stops.reduce((best, s) => (Math.abs(s - typed) <= Math.abs(best - typed) ? s : best));
  return { value, raw: { U32: value } };
}
