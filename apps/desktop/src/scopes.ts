// Pure maths for the live view scopes: waveform and false colour.

export const WAVEFORM_COLUMNS = 128;
export const WAVEFORM_ROWS = 64;

/** Counts per cell: column = x bucket, row = luminance bucket (0 bottom, 255 top). */
export function waveform(lum: Uint8ClampedArray | Float32Array | number[], width: number, height: number): Uint16Array {
  const out = new Uint16Array(WAVEFORM_COLUMNS * WAVEFORM_ROWS);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const l = Math.max(0, Math.min(255, lum[y * width + x]));
      const col = Math.min(WAVEFORM_COLUMNS - 1, Math.floor((x / width) * WAVEFORM_COLUMNS));
      const row = Math.min(WAVEFORM_ROWS - 1, Math.floor((l / 256) * WAVEFORM_ROWS));
      out[row * WAVEFORM_COLUMNS + col]++;
    }
  }
  return out;
}

export interface FalseColourBand {
  upTo: number;
  colour: string;
  label: string;
}

/** IRE-style bands; `upTo` is the highest 0–255 luminance in the band (IRE × 2.55, edges halfway). */
export const FALSE_COLOUR_BANDS: FalseColourBand[] = [
  { upTo: 19, colour: 'purple', label: 'crushed' },
  { upTo: 52, colour: 'blue', label: 'shadows' },
  { upTo: 108, colour: 'grey-blue', label: 'low' },
  { upTo: 121, colour: 'green', label: 'skin-dark' },
  { upTo: 133, colour: 'grey', label: 'mid' },
  { upTo: 149, colour: 'pink', label: 'skin' },
  { upTo: 197, colour: 'grey', label: 'high' },
  { upTo: 235, colour: 'yellow', label: 'highlights' },
  { upTo: 253, colour: 'orange', label: 'near-clip' },
  { upTo: 255, colour: 'red', label: 'clipped' },
];

export function bandFor(l: number): FalseColourBand {
  return FALSE_COLOUR_BANDS.find((b) => l <= b.upTo) ?? FALSE_COLOUR_BANDS[FALSE_COLOUR_BANDS.length - 1];
}

/** Fills `out` (RGBA) from the band colours; palette keys are band labels. */
export function falseColour(
  lum: Uint8ClampedArray | Float32Array | number[],
  out: Uint8ClampedArray,
  palette: Record<string, [number, number, number]>,
): void {
  for (let i = 0; i < lum.length; i++) {
    const [r, g, b] = palette[bandFor(lum[i]).label] ?? [0, 0, 0];
    out[i * 4] = r;
    out[i * 4 + 1] = g;
    out[i * 4 + 2] = b;
    out[i * 4 + 3] = 255;
  }
}
