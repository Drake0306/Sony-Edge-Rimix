// The card browser: a wide glass panel, opened from the Cameras panel's
// Browse card row, listing the active camera's card newest first. Each file
// can be downloaded into the Save to folder; its row shows the progress,
// then Saved (the file also lands in the filmstrip).

import type { Actions } from '../actions';
import { button, el } from '../dom';
import type { CardKind, CardObject } from '../ipc';
import { watch, type AppState, type AppStore, type CardState } from '../state';

export const TRANSFER_MODE_TEXT = 'This camera needs its own transfer mode for card browsing; not available yet';

const UNITS = ['KB', 'MB', 'GB', 'TB'];

/** A size in binary units: `512 B`, `2.0 MB`, `64 MB`, `1.2 GB` (one decimal under 10). */
export function sizeText(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 9.95 ? value.toFixed(1) : value.toFixed(0)} ${UNITS[unit]}`;
}

/** `2026-11-11T14:03:00` as `2026-11-11 14:03`; empty when unknown. */
export const dateText = (captured: string | null) => (captured ? captured.slice(0, 16).replace('T', ' ') : '');

/** Newest first by capture time (undated last), then by handle. */
export function sortCard(items: CardObject[]): CardObject[] {
  return [...items].sort((a, b) => {
    if (a.captured !== b.captured) {
      if (a.captured === null) return 1;
      if (b.captured === null) return -1;
      return a.captured < b.captured ? 1 : -1;
    }
    return b.handle - a.handle;
  });
}

const SVG = 'http://www.w3.org/2000/svg';

/** Line drawings in a 16-unit box: a picture, stacked frames, a movie camera, a page. */
const GLYPHS: Record<CardKind, string[]> = {
  Still: ['M2.5 3.5h11v9h-11z', 'M2.5 11l3.5-3.5 3 3 2-2 2.5 2.5'],
  Raw: ['M4.5 5.5h9v8h-9z', 'M2.5 10.5v-8h9'],
  Video: ['M1.5 4.5h9v7h-9z', 'M10.5 7l4-2.5v7l-4-2.5'],
  Other: ['M3.5 1.5h6l3 3v10h-9z', 'M9.5 1.5v3h3'],
};

function kindGlyph(kind: CardKind): HTMLElement {
  const wrap = el('span', `kind ${kind.toLowerCase()}`);
  wrap.setAttribute('role', 'img');
  wrap.setAttribute('aria-label', kind);
  const svg = document.createElementNS(SVG, 'svg');
  svg.setAttribute('viewBox', '0 0 16 16');
  svg.setAttribute('aria-hidden', 'true');
  for (const d of GLYPHS[kind]) {
    const path = document.createElementNS(SVG, 'path');
    path.setAttribute('d', d);
    svg.append(path);
  }
  wrap.append(svg);
  return wrap;
}

interface Row {
  el: HTMLElement;
  act: HTMLElement;
  /** What the action cell last showed, so unchanged rows are left alone. */
  shown: string;
}

export function mount(root: HTMLElement, store: AppStore, actions: Actions): () => void {
  const panel = el('div', 'cardpanel glass primary');
  panel.setAttribute('role', 'dialog');
  panel.setAttribute('aria-label', 'Browse card');
  panel.hidden = true;
  const head = el('div', 'card-head');
  const title = el('h2');
  const refresh = button('sbtn', 'Refresh');
  const close = button('sbtn', 'Close');
  head.append(title, refresh, close);
  const body = el('div', 'card-body');
  const list = el('ul', 'card-list');
  list.setAttribute('aria-label', 'Files on the card');
  panel.append(head, body);
  root.append(panel);

  refresh.addEventListener('click', () => void actions.cardRefresh());
  close.addEventListener('click', () => store.set({ cardOpen: false }));

  let rows = new Map<number, Row>();
  let listed: CardObject[] | null = null;

  /** The action cell's content for a file: button, progress, Saved or Too large. */
  function renderAction(row: Row, obj: CardObject, card: CardState) {
    const progress = card.downloads[obj.handle];
    const saved = obj.handle in card.done;
    const error = card.failed[obj.handle] ?? null;
    const key = JSON.stringify([obj.size_known, progress, saved, error]);
    if (key === row.shown) return;
    row.shown = key;
    const parts: HTMLElement[] = [];
    if (!obj.size_known) {
      parts.push(el('span', 'toolarge', 'Too large to download'));
    } else if (progress) {
      const pct = progress.total > 0 ? Math.min(100, Math.round((progress.received / progress.total) * 100)) : 0;
      const bar = el('span', 'bar');
      bar.setAttribute('role', 'progressbar');
      bar.setAttribute('aria-label', `Downloading ${obj.filename}`);
      bar.setAttribute('aria-valuemin', '0');
      bar.setAttribute('aria-valuemax', '100');
      bar.setAttribute('aria-valuenow', String(pct));
      const fill = el('i');
      fill.style.width = `${pct}%`;
      bar.append(fill);
      parts.push(bar, el('span', 'ptext', `${sizeText(progress.received)}/${sizeText(progress.total)}`));
    } else if (saved) {
      parts.push(el('span', 'saved', 'Saved'));
    } else {
      const b = button('sbtn', 'Download');
      b.setAttribute('aria-label', `Download ${obj.filename}`);
      b.addEventListener('click', () => void actions.cardDownload(obj.handle));
      parts.push(b);
    }
    row.act.replaceChildren(...parts);
    row.el.querySelector('.row-error')?.remove();
    if (error) {
      const line = el('div', 'row-error', error);
      line.setAttribute('role', 'alert');
      row.el.append(line);
    }
  }

  function buildRows(items: CardObject[]) {
    rows = new Map();
    list.replaceChildren(
      ...sortCard(items).map((obj) => {
        const li = el('li', 'card-row');
        li.dataset.handle = String(obj.handle);
        const name = el('span', 'name', obj.filename);
        name.title = obj.filename;
        const act = el('span', 'act');
        li.append(
          kindGlyph(obj.kind),
          name,
          el('span', 'size', obj.size_known ? sizeText(obj.bytes) : '4 GB+'),
          el('span', 'date', dateText(obj.captured)),
          act,
        );
        rows.set(obj.handle, { el: li, act, shown: '' });
        return li;
      }),
    );
  }

  function render(state: AppState) {
    const { card } = state;
    refresh.disabled = card.loading;
    let note: string | null = null;
    let failed = false;
    if (card.mode === 'needs-transfer-mode') note = TRANSFER_MODE_TEXT;
    else if (card.error) [note, failed] = [card.error, true];
    else if (card.items.length === 0) note = card.loading || card.mode === null ? 'Loading…' : 'No files on the card';
    if (note !== null) {
      const p = el('p', failed ? 'card-note error' : 'card-note', note);
      if (failed) p.setAttribute('role', 'alert');
      body.replaceChildren(p);
      listed = null;
      return;
    }
    if (listed !== card.items) {
      buildRows(card.items);
      listed = card.items;
    }
    if (list.parentElement !== body) body.replaceChildren(list);
    for (const obj of card.items) {
      const row = rows.get(obj.handle);
      if (row) renderAction(row, obj, card);
    }
  }

  const offOpen = watch(store, (s) => s.cardOpen, (open) => {
    panel.hidden = !open;
    if (!open) return;
    void actions.cardRefresh();
    close.focus();
  });
  // Rendered only while open; opening re-renders through the card's refresh.
  const offCard = watch(store, (s) => (s.cardOpen ? s.card : null), (_, s) => {
    if (s.cardOpen) render(s);
  });
  const offModel = watch(store, (s) => s.camera?.model ?? '', (model) => {
    title.textContent = model ? `Browse card · ${model}` : 'Browse card';
  });

  // Mounted after App settings and before the controls, so Escape closes
  // the App settings popover first, then this panel, then a camera popover.
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== 'Escape' || !store.get().cardOpen) return;
    e.stopImmediatePropagation();
    store.set({ cardOpen: false });
  };
  document.addEventListener('keydown', onKey);

  return () => {
    offOpen();
    offCard();
    offModel();
    document.removeEventListener('keydown', onKey);
    panel.remove();
  };
}
