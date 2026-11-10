// Live view: the camera's JPEG frames in an <img>, with focus frames, zebra,
// thirds grid, level and histogram drawn over it.

import type { Actions } from '../actions';
import { el } from '../dom';
import { liveViewFrame, liveViewMeta, type LiveViewMeta } from '../ipc';
import { watch, type AppStore } from '../state';

const FRAME_MS = 33; // at most 30 fps
const META_MS = 100;
const HIST_MS = 250;
const GRID = 64;
const BINS = 32;
const ZEBRA = 0.95 * 255;

const LEVEL_SVG =
  '<svg viewBox="0 0 120 16" aria-hidden="true"><line class="l1" x1="0" y1="8" x2="120" y2="8" stroke-width="1"/><line class="l2" x1="60" y1="2" x2="60" y2="14" stroke-width="1.5"/><line class="l3" x1="30" y1="4" x2="30" y2="12" stroke-width="1"/><line class="l3" x1="90" y1="4" x2="90" y2="12" stroke-width="1"/><line class="bub" x1="40" y1="8" x2="80" y2="8" stroke-width="2.5" stroke-linecap="round"/></svg>';
const HIST_SVG =
  '<svg viewBox="0 0 120 40" preserveAspectRatio="none" aria-hidden="true"><path d="M0 40 L120 40 Z"/></svg>';

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

export function mount(root: HTMLElement, store: AppStore, _actions: Actions): () => void {
  const scene = el('div', 'scene');
  const img = el('img', 'frame');
  img.alt = 'Live view';
  const waiting = el('p', 'waiting', 'Waiting for live view');
  scene.append(img, waiting);
  const thirds = el('div', 'thirds');
  const canvas = el('canvas', 'overlay');
  const label = el('span', 'focuslbl', 'Focus locked');
  [thirds, canvas, label].forEach((e) => e.setAttribute('aria-hidden', 'true'));
  label.hidden = true;
  const level = el('div', 'level glass capsule quiet');
  level.setAttribute('role', 'img');
  level.innerHTML = LEVEL_SVG;
  const levelText = el('span', 'num');
  level.append(levelText);
  const hist = el('div', 'hist glass deep');
  hist.setAttribute('role', 'img');
  hist.setAttribute('aria-label', 'Histogram');
  hist.innerHTML = HIST_SVG;
  hist.append(el('small', '', 'Histogram'));
  root.append(scene, thirds, canvas, label, level, hist);

  const bubble = level.querySelector('.bub')!;
  const histPath = hist.querySelector('path')!;
  const small = document.createElement('canvas');
  small.width = small.height = GRID;
  const sctx = small.getContext('2d', { willReadFrequently: true });
  const ctx = canvas.getContext('2d');
  const stripes = makeStripes();

  let meta: LiveViewMeta = { focus_frames: [], level_deg: null, fps: 0 };
  let lum: Float32Array | null = null;
  let current: string | null = null;
  let raf = 0;
  let last = -Infinity;
  let busy = false;
  let stopped = false;

  /** The image's rendered box inside the scene (object-fit: contain). */
  function box() {
    const cw = scene.clientWidth;
    const ch = scene.clientHeight;
    const nw = img.naturalWidth || 16;
    const nh = img.naturalHeight || 9;
    const s = Math.min(cw / nw, ch / nh);
    const w = nw * s;
    const h = nh * s;
    return { x: (cw - w) / 2, y: (ch - h) / 2, w, h, cw, ch };
  }

  function draw(): void {
    const b = box();
    Object.assign(thirds.style, { left: `${b.x}px`, top: `${b.y}px`, width: `${b.w}px`, height: `${b.h}px` });
    if (!ctx) return;
    const dpr = window.devicePixelRatio || 1;
    if (canvas.width !== Math.round(b.cw * dpr) || canvas.height !== Math.round(b.ch * dpr)) {
      canvas.width = Math.round(b.cw * dpr);
      canvas.height = Math.round(b.ch * dpr);
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, b.cw, b.ch);
    if (store.get().overlays.zebra && lum && stripes) {
      ctx.fillStyle = stripes;
      const cw = b.w / GRID;
      const chh = b.h / GRID;
      for (let i = 0; i < lum.length; i++) {
        if (lum[i] > ZEBRA) ctx.fillRect(b.x + (i % GRID) * cw, b.y + Math.floor(i / GRID) * chh, cw + 0.5, chh + 0.5);
      }
    }
    let locked: { x: number; y: number; w: number } | null = null;
    for (const f of meta.focus_frames) {
      const w = f.w * b.w;
      const h = f.h * b.h;
      const x = b.x + f.cx * b.w - w / 2;
      const y = b.y + f.cy * b.h - h / 2;
      const isLocked = f.state === 2;
      if (isLocked && !locked) locked = { x, y, w };
      corners(ctx, x, y, w, h, isLocked ? '#30D158' : 'rgba(255,255,255,.7)');
    }
    label.hidden = !locked;
    if (locked) Object.assign(label.style, { left: `${locked.x + locked.w / 2}px`, top: `${locked.y - 26}px` });
  }

  function loop(t: number): void {
    if (stopped) return;
    raf = requestAnimationFrame(loop);
    if (busy || t - last < FRAME_MS) return;
    last = t;
    busy = true;
    liveViewFrame()
      .then((buf) => {
        if (stopped || !buf || buf.byteLength === 0) return;
        const url = URL.createObjectURL(new Blob([buf], { type: 'image/jpeg' }));
        const prev = current;
        current = url;
        img.onload = () => {
          if (prev) URL.revokeObjectURL(prev);
          waiting.hidden = true;
          draw();
        };
        img.src = url;
      })
      .catch(() => {})
      .finally(() => (busy = false));
  }

  let metaTimer = 0;
  const pollMeta = async () => {
    try {
      meta = await liveViewMeta();
      const deg = meta.level_deg;
      level.hidden = deg === null;
      if (deg !== null) {
        bubble.setAttribute('transform', `rotate(${-deg} 60 8)`);
        levelText.textContent = `${Math.abs(deg).toFixed(1)}°`;
        level.setAttribute('aria-label', `Level: ${Math.abs(deg).toFixed(1)} degrees ${deg < 0 ? 'right' : 'left'}`);
      }
      const status = store.get().status;
      if (Math.round(meta.fps) !== Math.round(status.fps)) store.set({ status: { ...status, fps: meta.fps } });
      draw();
    } catch {
      // The next poll tries again.
    }
    if (!stopped) metaTimer = window.setTimeout(pollMeta, META_MS);
  };

  const histTimer = window.setInterval(() => {
    if (!sctx || !img.naturalWidth) return;
    sctx.drawImage(img, 0, 0, GRID, GRID);
    const result = luminance(sctx.getImageData(0, 0, GRID, GRID).data);
    lum = result.lum;
    histPath.setAttribute('d', histogramPath(result.bins));
  }, HIST_MS);

  level.hidden = true;
  void pollMeta();
  raf = requestAnimationFrame(loop);
  const off = watch(store, (s) => s.overlays, (o) => {
    thirds.hidden = !o.thirds;
    draw();
  });
  window.addEventListener('resize', draw);

  return () => {
    stopped = true;
    cancelAnimationFrame(raf);
    window.clearTimeout(metaTimer);
    window.clearInterval(histTimer);
    window.removeEventListener('resize', draw);
    off();
    if (current) URL.revokeObjectURL(current);
    [scene, thirds, canvas, label, level, hist].forEach((e) => e.remove());
  };
}

/** Corner brackets like the mockup's focus frame. */
function corners(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, colour: string) {
  const l = Math.min(20, w / 3, h / 3);
  ctx.strokeStyle = colour;
  ctx.lineWidth = 2;
  ctx.shadowColor = 'rgba(0,0,0,.5)';
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
function makeStripes(): CanvasPattern | null {
  const tile = document.createElement('canvas');
  tile.width = tile.height = 8;
  const c = tile.getContext('2d');
  if (!c) return null;
  c.fillStyle = 'rgba(0,0,0,.55)';
  c.fillRect(0, 0, 8, 8);
  c.strokeStyle = 'rgba(255,255,255,.85)';
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
