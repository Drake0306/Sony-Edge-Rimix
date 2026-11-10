import { describe, expect, test } from 'vitest';
import { FALSE_COLOUR_BANDS, WAVEFORM_COLUMNS, WAVEFORM_ROWS, bandFor, falseColour, waveform } from './scopes';

describe('waveform', () => {
  test('an all-black frame puts every count in row 0', () => {
    const out = waveform(new Array(16).fill(0), 4, 4);
    expect(out.length).toBe(WAVEFORM_COLUMNS * WAVEFORM_ROWS);
    let row0 = 0;
    let total = 0;
    out.forEach((n, i) => {
      total += n;
      if (Math.floor(i / WAVEFORM_COLUMNS) === 0) row0 += n;
    });
    expect(total).toBe(16);
    expect(row0).toBe(16);
  });

  test('left black and right white fills columns 0..63 at row 0 and 64..127 at the top row', () => {
    const lum: number[] = [];
    // One pixel per column, as in the 128-wide live view sample.
    for (let y = 0; y < 2; y++) for (let x = 0; x < 128; x++) lum.push(x < 64 ? 0 : 255);
    const out = waveform(lum, 128, 2);
    const at = (col: number, row: number) => out[row * WAVEFORM_COLUMNS + col];
    for (let c = 0; c < 64; c++) {
      expect(at(c, 0)).toBeGreaterThan(0);
      expect(at(c, WAVEFORM_ROWS - 1)).toBe(0);
    }
    for (let c = 64; c < 128; c++) {
      expect(at(c, WAVEFORM_ROWS - 1)).toBeGreaterThan(0);
      expect(at(c, 0)).toBe(0);
    }
  });
});

describe('false colour', () => {
  test('bandFor follows the table', () => {
    expect(bandFor(0).label).toBe('crushed');
    expect(bandFor(128).label).toBe('mid');
    expect(bandFor(140).label).toBe('skin');
    expect(bandFor(255).label).toBe('clipped');
    expect(FALSE_COLOUR_BANDS[FALSE_COLOUR_BANDS.length - 1].upTo).toBe(255);
  });

  test('falseColour writes the band colour with alpha 255', () => {
    const palette: Record<string, [number, number, number]> = {};
    FALSE_COLOUR_BANDS.forEach((b, i) => (palette[b.label] = [i * 10, i * 5, 7]));
    const out = new Uint8ClampedArray(8);
    falseColour([0, 255], out, palette);
    expect([...out]).toEqual([0, 0, 7, 255, 90, 45, 7, 255]);
  });
});
