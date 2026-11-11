// The last six captures, newest first, with files still arriving from the
// camera shown on top with a progress bar. While several cameras are
// connected, each tile names the camera it came from; files the camera
// pushed over SFTP always say Received.

import type { Actions } from '../actions';
import { el } from '../dom';
import { readThumbnail } from '../ipc';
import {
  cameraCount,
  MAX_CAPTURES,
  SFTP_CAMERA_ID,
  transferCamera,
  transferName,
  watch,
  type AppStore,
  type CaptureItem,
  type Transfer,
} from '../state';

const stem = (name: string) => name.replace(/\.[^.]+$/, '');
const ext = (name: string) => (/\.([^.]+)$/.exec(name)?.[1] ?? '').toUpperCase();
const isJpeg = (name: string) => ['JPG', 'JPEG'].includes(ext(name));

export function mount(root: HTMLElement, store: AppStore, _actions: Actions): () => void {
  const film = el('aside', 'film glass deep');
  film.setAttribute('aria-label', 'Last six captures');
  const h = el('div', 'h');
  h.append(el('span', '', 'Last captures'));
  // Everything arriving, as one line and bar above the tiles.
  const total = el('div', 'total');
  total.setAttribute('role', 'status');
  const what = el('span', 'what');
  const pctText = el('span', 'pct');
  const line = el('div', 'line');
  line.append(what, pctText);
  const bar = el('div', 'bar');
  bar.setAttribute('role', 'progressbar');
  bar.setAttribute('aria-valuemin', '0');
  bar.setAttribute('aria-valuemax', '100');
  bar.setAttribute('aria-label', 'Files being copied to this computer');
  const fill = el('i');
  bar.append(fill);
  total.append(line, bar);
  total.hidden = true;
  const strip = el('div', 'strip');
  film.append(h, total, strip);
  root.append(film);

  /** Object URLs of thumbnails by file path; '' while loading or when there is none. */
  const urls = new Map<string, string>();

  function picture(item: CaptureItem): HTMLElement {
    const pic = el('span', 'pic');
    if (!isJpeg(item.filename)) {
      pic.append(el('span', 'rawtile', ext(item.filename) || 'File'));
      return pic;
    }
    const url = urls.get(item.path);
    if (url) {
      const img = el('img');
      img.alt = '';
      img.src = url;
      pic.append(img);
    } else if (url === undefined) {
      urls.set(item.path, '');
      readThumbnail(item.path)
        .then((bytes) => {
          if (!urls.has(item.path)) return;
          urls.set(item.path, URL.createObjectURL(new Blob([bytes], { type: 'image/jpeg' })));
          render();
        })
        .catch(() => {});
    }
    return pic;
  }

  function tile(name: string, pic: HTMLElement, cameraLabel: string | null, transfer?: Transfer): HTMLElement {
    const t = el('div', 'thumb');
    t.append(pic);
    if (cameraLabel) t.append(el('span', 'cam', cameraLabel));
    const n = el('span', 'n', stem(name));
    if (transfer) {
      const pct = transfer.total > 0 ? Math.round((transfer.received / transfer.total) * 100) : 0;
      const pb = el('span', 'pb');
      pb.setAttribute('aria-hidden', 'true');
      const bar = el('i');
      bar.style.width = `${pct}%`;
      pb.append(bar);
      t.append(pb);
      n.classList.add('busy');
      n.append(el('span', '', `transferring ${pct}%`));
      t.setAttribute('aria-label', `${stem(name)}${cameraLabel ? ` from ${cameraLabel}` : ''}, transferring ${pct}%`);
    } else {
      t.setAttribute('aria-label', `${stem(name)}${cameraLabel ? ` from ${cameraLabel}` : ''}`);
    }
    t.append(n);
    return t;
  }

  function render(): void {
    const state = store.get();
    const { captures, transfers } = state;
    const several = cameraCount(state) > 1;
    const labelOf = (id: string | null) => (several && id !== null ? (state.cameras[id]?.model ?? null) : null);
    const kept = new Set(captures.map((c) => c.path));
    for (const [path, url] of urls) {
      if (kept.has(path)) continue;
      if (url) URL.revokeObjectURL(url);
      urls.delete(path);
    }
    const arriving = Object.entries(transfers);
    total.hidden = arriving.length === 0;
    if (arriving.length > 0) {
      const sum = (pick: (t: Transfer) => number) => arriving.reduce((n, [, t]) => n + pick(t), 0);
      const all = sum((t) => t.total);
      const pct = all > 0 ? Math.min(100, Math.round((sum((t) => t.received) / all) * 100)) : 0;
      const name = arriving.length === 1 ? transferName(arriving[0][0]) : `${arriving.length} files`;
      what.textContent = `Copying ${name} to this computer`;
      pctText.textContent = `${pct}%`;
      bar.setAttribute('aria-valuenow', String(pct));
      fill.style.width = `${pct}%`;
    }
    const busy = arriving.map(([key, tr]) =>
      tile(transferName(key), el('span', 'pic'), labelOf(transferCamera(key)), tr),
    );
    const captionOf = (c: CaptureItem) => (several || c.cameraId === SFTP_CAMERA_ID ? c.cameraLabel || null : null);
    const done = captures.map((c) => tile(c.filename, picture(c), captionOf(c)));
    const tiles = [...busy, ...done].slice(0, MAX_CAPTURES);
    strip.replaceChildren(...(tiles.length ? tiles : [el('p', 'empty', 'Shots appear here')]));
  }

  const offs = [
    watch(store, (s) => s.captures, render),
    watch(store, (s) => s.transfers, render),
    watch(store, (s) => cameraCount(s) > 1, render),
  ];
  return () => {
    offs.forEach((off) => off());
    for (const url of urls.values()) if (url) URL.revokeObjectURL(url);
    film.remove();
  };
}
