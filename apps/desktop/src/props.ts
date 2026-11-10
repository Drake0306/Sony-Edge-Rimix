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

/** White balance raw value meaning "use the colour temperature". */
const WB_COLOUR_TEMP = 0x8007;

export const findProp = (camera: CameraState | null, code: number): PropertySummary | undefined =>
  camera?.properties.find((p) => p.code === code);

export const sameRaw = (a: PtpValue, b: PtpValue): boolean => JSON.stringify(a) === JSON.stringify(b);

export function isColourTemp(camera: CameraState | null): boolean {
  const wb = findProp(camera, PROP.whiteBalance);
  return !!wb && 'U16' in wb.raw && wb.raw.U16 === WB_COLOUR_TEMP && !!findProp(camera, PROP.colourTemp);
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
