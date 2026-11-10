// Live view: the camera's JPEG frames in an <img>, with focus frames, zebra,
// thirds grid, level and histogram drawn over it.

import type { Actions } from '../actions';
import { el } from '../dom';
import { liveViewFrame, liveViewMeta, type LiveViewMeta } from '../ipc';
import { watch, type AppStore } from '../state';
import { WAVEFORM_COLUMNS, WAVEFORM_ROWS, falseColour, waveform } from '../scopes';
import { colours, corners, createFrameSwap, histogramPath, luminance, makeStripes } from './liveviewParts';

const FRAME_MS = 33; // at most 30 fps
const META_MS = 100;
const HIST_MS = 250;
const GRID = 64;
const SCOPE_W = 128;
const SCOPE_H = 72;
const ZEBRA = 0.95 * 255;

const LEVEL_SVG =
  '<svg viewBox="0 0 120 16" aria-hidden="true"><line class="l1" x1="0" y1="8" x2="120" y2="8" stroke-width="1"/><line class="l2" x1="60" y1="2" x2="60" y2="14" stroke-width="1.5"/><line class="l3" x1="30" y1="4" x2="30" y2="12" stroke-width="1"/><line class="l3" x1="90" y1="4" x2="90" y2="12" stroke-width="1"/><line class="bub" x1="40" y1="8" x2="80" y2="8" stroke-width="2.5" stroke-linecap="round"/></svg>';
const HIST_SVG =
  '<svg viewBox="0 0 120 40" preserveAspectRatio="none" aria-hidden="true"><path d="M0 40 L120 40 Z"/></svg>';

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
  const fcCanvas = el('canvas', 'falsecolour');
  fcCanvas.width = SCOPE_W;
  fcCanvas.height = SCOPE_H;
  fcCanvas.hidden = true;
  fcCanvas.setAttribute('aria-hidden', 'true');
  const wave = el('div', 'wave glass deep');
  wave.setAttribute('role', 'img');
  wave.setAttribute('aria-label', 'Waveform');
  wave.hidden = true;
  const waveCanvas = el('canvas');
  waveCanvas.width = WAVEFORM_COLUMNS;
  waveCanvas.height = WAVEFORM_ROWS;
  wave.append(waveCanvas);
  root.append(scene, fcCanvas, thirds, canvas, label, level, hist, wave);

  const bubble = level.querySelector('.bub')!;
  const histPath = hist.querySelector('path')!;
  const small = document.createElement('canvas');
  small.width = small.height = GRID;
  const sctx = small.getContext('2d', { willReadFrequently: true });
  const ctx = canvas.getContext('2d');
  const scopeSmall = document.createElement('canvas');
  scopeSmall.width = SCOPE_W;
  scopeSmall.height = SCOPE_H;
  const scopeCtx = scopeSmall.getContext('2d', { willReadFrequently: true });
  const fcCtx = fcCanvas.getContext('2d');
  const waveCtx = waveCanvas.getContext('2d');
  const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
  const SCOPE_MS = reduced ? HIST_MS * 4 : HIST_MS;
  const fcImage = fcCtx ? fcCtx.createImageData(SCOPE_W, SCOPE_H) : null;
  // Colours are re-read on every draw so a theme change shows at once; the
  // zebra pattern is rebuilt only when its colours change.
  let stripes: CanvasPattern | null = null;
  let stripesKey = '';

  let meta: LiveViewMeta = { focus_frames: [], level_deg: null, fps: 0 };
  let lum: Float32Array | null = null;
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
    Object.assign(fcCanvas.style, { left: `${b.x}px`, top: `${b.y}px`, width: `${b.w}px`, height: `${b.h}px` });
    Object.assign(thirds.style, { left: `${b.x}px`, top: `${b.y}px`, width: `${b.w}px`, height: `${b.h}px` });
    if (!ctx) return;
    const dpr = window.devicePixelRatio || 1;
    if (canvas.width !== Math.round(b.cw * dpr) || canvas.height !== Math.round(b.ch * dpr)) {
      canvas.width = Math.round(b.cw * dpr);
      canvas.height = Math.round(b.ch * dpr);
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, b.cw, b.ch);
    const palette = colours();
    const key = `${palette.zebraDark} ${palette.zebraLight}`;
    if (key !== stripesKey) {
      stripesKey = key;
      stripes = makeStripes(palette);
    }
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
      corners(ctx, x, y, w, h, isLocked ? palette.lock : palette.idle, palette.shadow);
    }
    label.hidden = !locked;
    if (locked) Object.assign(label.style, { left: `${locked.x + locked.w / 2}px`, top: `${locked.y - 26}px` });
  }

  const swap = createFrameSwap(img, () => {
    waiting.hidden = true;
    draw();
  });

  function loop(t: number): void {
    if (stopped) return;
    raf = requestAnimationFrame(loop);
    if (busy || t - last < FRAME_MS) return;
    last = t;
    busy = true;
    liveViewFrame()
      .then((buf) => {
        if (stopped || !buf || buf.byteLength === 0) return;
        swap.show(URL.createObjectURL(new Blob([buf], { type: 'image/jpeg' })));
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

  function drawScopes(): void {
    const o = store.get().overlays;
    if ((!o.waveform && !o.falseColour) || !scopeCtx || !img.naturalWidth) return;
    scopeCtx.drawImage(img, 0, 0, SCOPE_W, SCOPE_H);
    const { lum: l } = luminance(scopeCtx.getImageData(0, 0, SCOPE_W, SCOPE_H).data);
    const palette = colours();
    if (o.falseColour && fcCtx && fcImage) {
      falseColour(l, fcImage.data, palette.falseColour);
      fcCtx.putImageData(fcImage, 0, 0);
    }
    if (o.waveform && waveCtx) {
      const cells = waveform(l, SCOPE_W, SCOPE_H);
      const cols = new Uint16Array(WAVEFORM_COLUMNS);
      cells.forEach((n, i) => (cols[i % WAVEFORM_COLUMNS] = Math.max(cols[i % WAVEFORM_COLUMNS], n)));
      waveCtx.fillStyle = palette.scopeBg;
      waveCtx.fillRect(0, 0, WAVEFORM_COLUMNS, WAVEFORM_ROWS);
      waveCtx.fillStyle = palette.scopeTrace;
      for (let i = 0; i < cells.length; i++) {
        if (!cells[i]) continue;
        const col = i % WAVEFORM_COLUMNS;
        waveCtx.globalAlpha = Math.max(0.15, cells[i] / cols[col]);
        waveCtx.fillRect(col, WAVEFORM_ROWS - 1 - Math.floor(i / WAVEFORM_COLUMNS), 1, 1);
      }
      waveCtx.globalAlpha = 1;
    }
  }
  const scopeTimer = window.setInterval(drawScopes, SCOPE_MS);

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
    fcCanvas.hidden = !o.falseColour;
    wave.hidden = !o.waveform;
    draw();
  });
  window.addEventListener('resize', draw);

  return () => {
    stopped = true;
    cancelAnimationFrame(raf);
    window.clearTimeout(metaTimer);
    window.clearInterval(histTimer);
    window.clearInterval(scopeTimer);
    window.removeEventListener('resize', draw);
    off();
    swap.dispose();
    [scene, fcCanvas, thirds, canvas, label, level, hist, wave].forEach((e) => e.remove());
  };
}

