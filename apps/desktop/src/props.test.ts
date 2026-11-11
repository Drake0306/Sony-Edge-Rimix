import { describe, expect, test } from 'vitest';
import type { CameraState, PropertySummary, PtpValue } from './ipc';
import { isColourTemp, isoStops, nearestIso, PROP } from './props';

const u32 = (n: number): PtpValue => ({ U32: n });
const iso = (current: number): PropertySummary => ({
  code: PROP.iso,
  label: `ISO ${current}`,
  short: String(current),
  raw: u32(current),
  writable: true,
  enabled: 'On',
  // Auto first, then stops out of order, then a multi-frame noise reduction value.
  choices: [0xffffff, 100, 125, 160, 200, 50, 64, 80, 250, 320, 12800, 32000, 0x10000320].map((n) => ({
    raw: u32(n),
    label: `ISO ${n}`,
  })),
});

describe('custom ISO', () => {
  test('the stops are the plain values in order: no Auto, no noise reduction variants', () => {
    expect(isoStops(iso(100))).toEqual([50, 64, 80, 100, 125, 160, 200, 250, 320, 12800, 32000]);
    expect(isoStops(undefined)).toEqual([]);
  });

  test('a typed value lands on the nearest stop the camera has', () => {
    expect(nearestIso(iso(100), 150)).toEqual({ value: 160, raw: u32(160) });
    expect(nearestIso(iso(100), 140)).toEqual({ value: 125, raw: u32(125) });
    expect(nearestIso(iso(100), 200)).toEqual({ value: 200, raw: u32(200) });
    // Halfway goes up.
    expect(nearestIso(iso(100), 225)).toEqual({ value: 250, raw: u32(250) });
  });

  test('values outside the range stop at its ends, and nonsense is refused', () => {
    expect(nearestIso(iso(100), 5)).toEqual({ value: 50, raw: u32(50) });
    expect(nearestIso(iso(100), 999999)).toEqual({ value: 32000, raw: u32(32000) });
    expect(nearestIso(iso(100), Number.NaN)).toBeNull();
    expect(nearestIso(iso(100), -3)).toBeNull();
    expect(nearestIso(undefined, 100)).toBeNull();
  });
});

describe('colour temperature', () => {
  const camera = (wb: number): CameraState =>
    ({
      properties: [
        { code: PROP.whiteBalance, label: '', short: '', raw: { U16: wb }, writable: true, enabled: 'On', choices: [] },
        { code: PROP.colourTemp, label: '', short: '5200', raw: { U16: 5200 }, writable: true, enabled: 'On', choices: [] },
      ],
    }) as unknown as CameraState;

  test('both codes cameras use for it count', () => {
    expect(isColourTemp(camera(0x8012))).toBe(true);
    expect(isColourTemp(camera(0x8007))).toBe(true);
    expect(isColourTemp(camera(0x0004))).toBe(false);
  });
});
