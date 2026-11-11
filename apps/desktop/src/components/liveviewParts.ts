// Pieces of the live view that do not need the loop: the frame swap that
// revokes object URLs, histogram maths, and the canvas drawing helpers.

const BINS = 32;

/** 32-bin luminance histogram of RGBA pixels, and the luminance of each pixel. */
export function luminance(rgba: Uint8ClampedArray): { bins: number[]; lum: Float32Array } {
  const bins = new Array<number>(BINS).fill(0);
  const lum = new Float32Array(rgba.length / 4);
  for (let i = 0; i < lum.length; i++) {
    const y = 0.2126 * rgba[i * 4] + 0.7152 * rgba[i * 4 + 1] + 0.0722 * rgba[i * 4 + 2];
    lum[i] = y;
    bins[Math.min(BINS - 1, Math.floor((y / 256) * BINS))]++;
  }
  return { bins, lum };
}

/** An SVG area path for the histogram in a 120×40 box. */
export function histogramPath(bins: number[]): string {
  const max = Math.max(1, ...bins);
  const step = 120 / (bins.length - 1);
  const pts = bins.map((b, i) => `L${(i * step).toFixed(1)} ${(40 - (b / max) * 38).toFixed(1)}`);
  return `M0 40 ${pts.join(' ')} L120 40 Z`;
}

/**
 * Shows object URLs in `img` one after another and revokes every URL that is
 * no longer on screen: the old frame once the new one loads, a frame that
 * fails to decode, and a frame replaced before it finished loading.
 */
export function createFrameSwap(img: HTMLImageElement, onShown: () => void) {
  let shown: string | null = null;
  let loading: string | null = null;
  img.onload = () => {
    if (!loading) return;
    if (shown) URL.revokeObjectURL(shown);
    shown = loading;
    loading = null;
    onShown();
  };
  img.onerror = () => {
    if (!loading) return;
    URL.revokeObjectURL(loading);
    loading = null;
  };
  return {
    show(url: string): void {
      if (loading) URL.revokeObjectURL(loading);
      loading = url;
      img.src = url;
    },
    dispose(): void {
      for (const url of [shown, loading]) if (url) URL.revokeObjectURL(url);
      shown = loading = null;
    },
  };
}

/** Canvas colours, read from the theme tokens. */
export function colours() {
  const css = getComputedStyle(document.documentElement);
  const get = (name: string) => css.getPropertyValue(name).trim();
  return {
    lock: get('--lock'),
    idle: get('--focus-idle'),
    shadow: get('--footage-shadow'),
    zebraDark: get('--zebra-dark'),
    zebraLight: get('--zebra-light'),
    scopeTrace: get('--scope-trace'),
    scopeBg: get('--scope-bg'),
    falseColour: Object.fromEntries(
      Object.entries(FC_TOKENS).map(([label, token]) => [label, parseHex(get(token))]),
    ) as Record<string, [number, number, number]>,
  };
}

const FC_TOKENS: Record<string, string> = {
  crushed: '--fc-crushed',
  shadows: '--fc-shadows',
  low: '--fc-low',
  'skin-dark': '--fc-skin-dark',
  mid: '--fc-mid',
  skin: '--fc-skin',
  high: '--fc-high',
  highlights: '--fc-highlights',
  'near-clip': '--fc-near-clip',
  clipped: '--fc-clipped',
};

/** `#rgb` or `#rrggbb` to [r, g, b]; anything else is black. */
export function parseHex(hex: string): [number, number, number] {
  let h = hex.trim().replace(/^#/, '');
  if (h.length === 3) h = [...h].map((c) => c + c).join('');
  if (!/^[0-9a-f]{6}$/i.test(h)) return [0, 0, 0];
  return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
}

export type Colours = ReturnType<typeof colours>;

/** Corner brackets like the mockup's focus frame. */
export function corners(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  colour: string,
  shadow: string,
) {
  const l = Math.min(20, w / 3, h / 3);
  ctx.strokeStyle = colour;
  ctx.lineWidth = 2;
  ctx.shadowColor = shadow;
  ctx.shadowBlur = 2;
  ctx.beginPath();
  for (const [cx, cy, dx, dy] of [[x, y, 1, 1], [x + w, y, -1, 1], [x, y + h, 1, -1], [x + w, y + h, -1, -1]]) {
    ctx.moveTo(cx + dx * l, cy);
    ctx.lineTo(cx, cy);
    ctx.lineTo(cx, cy + dy * l);
  }
  ctx.stroke();
  ctx.shadowBlur = 0;
}

/** Diagonal black and white stripes for zebra. */
export function makeStripes(palette: Colours): CanvasPattern | null {
  const tile = document.createElement('canvas');
  tile.width = tile.height = 8;
  const c = tile.getContext('2d');
  if (!c) return null;
  c.fillStyle = palette.zebraDark;
  c.fillRect(0, 0, 8, 8);
  c.strokeStyle = palette.zebraLight;
  c.lineWidth = 2.5;
  c.beginPath();
  c.moveTo(-2, 10);
  c.lineTo(10, -2);
  c.moveTo(6, 10);
  c.lineTo(10, 6);
  c.moveTo(-2, 2);
  c.lineTo(2, -2);
  c.stroke();
  return c.createPattern(tile, 'repeat');
}

/** A frame with rounded corners, as round as a face: solid, or dashed for a subject being tracked. */
export function rounded(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  colour: string,
  shadow: string,
  dashed: boolean,
) {
  ctx.save();
  ctx.strokeStyle = colour;
  ctx.lineWidth = 2;
  ctx.shadowColor = shadow;
  ctx.shadowBlur = 2;
  ctx.setLineDash(dashed ? [7, 5] : []);
  const r = Math.min(w, h) * (dashed ? 0.12 : 0.42);
  ctx.beginPath();
  if (typeof ctx.roundRect === 'function') ctx.roundRect(x, y, w, h, r);
  else ctx.rect(x, y, w, h);
  ctx.stroke();
  ctx.restore();
}
