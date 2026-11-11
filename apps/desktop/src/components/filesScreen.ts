// The files of a camera in Send to Smartphone: its folders, a small picture
// of every file, a larger one to look at, and Import. No live view and no
// camera controls: in this mode the camera only serves its files.

import type { Actions } from '../actions';
import { button, el } from '../dom';
import { filesPicture, type FileEntry } from '../ipc';
import { watch, type AppStore, type FilesState } from '../state';
import { sizeText } from './cardBrowser';
import { destinationText } from './sidePanels';

/** How many small pictures are asked of the camera at once: its server is a small one. */
const AT_ONCE = 2;

export const CHOSEN_ON_CAMERA = 'The pictures chosen on the camera';
export const WHOLE_CARD = 'Everything on the card';
export const NOTHING_CHOSEN =
  'The camera lists nothing here. Choose pictures on the camera with Send to Smartphone, then connect again.';
export const PREVIEW_ONLY = 'The camera sends a JPEG copy of this file, not the file itself';

/** What the camera calls the file's kind, from its name. */
export function kindOf(title: string): 'picture' | 'raw' | 'movie' | 'file' {
  const ext = title.slice(title.lastIndexOf('.') + 1).toUpperCase();
  if (['JPG', 'JPEG', 'HIF', 'HEIF'].includes(ext)) return 'picture';
  if (['ARW', 'SRF', 'SR2'].includes(ext)) return 'raw';
  if (['MP4', 'MTS', 'MOV', 'XML'].includes(ext)) return ext === 'XML' ? 'file' : 'movie';
  return 'file';
}

/** `2.0 MB · 6000x4000`, or what is known of it. */
export function metaText(entry: FileEntry): string {
  const parts = [entry.bytes !== null ? sizeText(entry.bytes) : '', entry.resolution ?? ''].filter(Boolean);
  if (!entry.original && entry.file) parts.push('JPEG copy');
  return parts.join(' · ');
}

export function mount(root: HTMLElement, store: AppStore, actions: Actions): () => void {
  const screen = el('div', 'files');

  const bar = el('header', 'files-bar glass primary');
  const done = button('sbtn', 'Done');
  done.title = 'The camera leaves Send to Smartphone';
  const heading = el('div', 'files-head');
  const title = el('h1');
  const sub = el('p');
  heading.append(title, sub);
  const where = el('span', 'where');
  const choose = button('sbtn', 'Choose folder');
  const importAll = button('pbtn', 'Import all');
  bar.append(done, heading, where, choose, importAll);

  const crumbs = el('nav', 'files-path');
  crumbs.setAttribute('aria-label', 'Folder');
  const grid = el('ul', 'files-grid');
  grid.setAttribute('aria-label', 'Files on the camera');
  const note = el('p', 'files-note');
  note.setAttribute('role', 'status');

  // One file opened large.
  const viewer = el('div', 'files-view');
  viewer.setAttribute('role', 'dialog');
  viewer.setAttribute('aria-label', 'Picture');
  viewer.hidden = true;
  const big = el('img');
  big.alt = '';
  const viewName = el('b');
  const viewMeta = el('span', 'meta');
  const viewAct = el('span', 'act');
  const previous = button('sbtn', 'Previous');
  const next = button('sbtn', 'Next');
  const close = button('sbtn', 'Close');
  const viewBar = el('div', 'view-bar');
  viewBar.append(viewName, viewMeta, viewAct, previous, next, close);
  viewer.append(big, viewBar);

  screen.append(bar, crumbs, note, grid, viewer);
  root.append(screen);

  done.addEventListener('click', () => void actions.filesDone());
  importAll.addEventListener('click', () => void actions.filesImportAll());
  choose.addEventListener('click', async () => {
    const folder = await actions.pickFolder();
    const s = store.get().settings;
    if (folder && s) void actions.saveSettings({ ...s, destinationBase: folder });
  });
  close.addEventListener('click', () => actions.filesView(null));

  // Small pictures, fetched a few at a time and kept while the screen is up.
  const pictures = new Map<string, string>();
  const waiting: { link: string; show: (url: string) => void }[] = [];
  let fetching = 0;
  let stopped = false;
  function pump(): void {
    while (!stopped && fetching < AT_ONCE && waiting.length > 0) {
      const { link, show } = waiting.shift()!;
      const have = pictures.get(link);
      if (have) {
        show(have);
        continue;
      }
      fetching += 1;
      filesPicture(link)
        .then((bytes) => {
          if (stopped) return;
          const url = URL.createObjectURL(new Blob([bytes], { type: 'image/jpeg' }));
          pictures.set(link, url);
          show(url);
        })
        .catch(() => {})
        .finally(() => {
          fetching -= 1;
          pump();
        });
    }
  }
  /** Shows the picture at `link` in `img` once it has come; a newer wish for the same `img` wins. */
  function picture(img: HTMLImageElement, link: string | null, first = false): void {
    img.dataset.link = link ?? '';
    img.removeAttribute('src');
    if (!link) return;
    const show = (url: string) => {
      if (img.dataset.link === link) img.src = url;
    };
    if (first) waiting.unshift({ link, show });
    else waiting.push({ link, show });
    pump();
  }

  /** The Import button of a file, or how far it is, or that it is saved. */
  function action(entry: FileEntry, files: FilesState): HTMLElement[] {
    const progress = files.progress[entry.id];
    if (progress) {
      const pct = progress.total ? Math.min(100, Math.round((progress.received / progress.total) * 100)) : 0;
      const barEl = el('span', 'bar');
      barEl.setAttribute('role', 'progressbar');
      barEl.setAttribute('aria-label', `Importing ${entry.title}`);
      barEl.setAttribute('aria-valuemin', '0');
      barEl.setAttribute('aria-valuemax', '100');
      barEl.setAttribute('aria-valuenow', String(pct));
      const fill = el('i');
      fill.style.width = `${pct}%`;
      barEl.append(fill);
      return [barEl];
    }
    if (entry.id in files.saved) {
      const saved = el('span', 'saved', 'Imported');
      saved.title = files.saved[entry.id];
      return [saved];
    }
    if (!entry.file) return [el('span', 'none', 'Not offered')];
    const b = button('sbtn', 'Import', `Import ${entry.title}`);
    if (!entry.original) b.title = PREVIEW_ONLY;
    b.addEventListener('click', (e) => {
      e.stopPropagation();
      void actions.filesImport(entry);
    });
    const failed = files.failed[entry.id];
    if (!failed) return [b];
    const why = el('span', 'failed', failed);
    why.setAttribute('role', 'alert');
    return [b, why];
  }

  let listed: FileEntry[] | null = null;
  const acts = new Map<string, HTMLElement>();
  function draw(files: FilesState | null): void {
    if (!files) return;
    title.textContent = `Files on ${files.info.model || 'the camera'}`;
    sub.textContent = files.info.root === 'PushRoot' ? CHOSEN_ON_CAMERA : WHOLE_CARD;
    crumbs.replaceChildren(
      ...files.path.map((p, i) => {
        const last = i === files.path.length - 1;
        const b = button(last ? 'on' : '', p.title);
        b.disabled = last;
        if (last) b.setAttribute('aria-current', 'page');
        b.addEventListener('click', () => void actions.filesEnter(i));
        return b;
      }),
    );
    const some = files.items.some((f) => !f.folder && f.file && !(f.id in files.saved));
    importAll.disabled = !some || Object.keys(files.progress).length > 0;
    note.className = files.error ? 'files-note error' : 'files-note';
    note.textContent = files.error ?? (files.loading ? 'Reading the camera…' : files.items.length === 0 ? NOTHING_CHOSEN : '');
    note.hidden = note.textContent === '';
    if (listed !== files.items) {
      listed = files.items;
      acts.clear();
      waiting.length = 0;
      grid.replaceChildren(
        ...files.items.map((entry) => {
          const li = el('li', entry.folder ? 'tile folder' : `tile ${kindOf(entry.title)}`);
          li.dataset.id = entry.id;
          const open = button('pic');
          open.setAttribute('aria-label', entry.folder ? `Open ${entry.title}` : `Look at ${entry.title}`);
          if (entry.folder) {
            open.append(el('span', 'glyph'));
            open.addEventListener('click', () => void actions.filesEnter(entry));
          } else {
            const img = el('img');
            img.alt = '';
            img.loading = 'lazy';
            open.append(img, el('span', 'kind', kindOf(entry.title) === 'raw' ? 'RAW' : kindOf(entry.title) === 'movie' ? 'Movie' : ''));
            picture(img, entry.thumb);
            open.addEventListener('click', () => actions.filesView(entry.id));
          }
          const name = el('span', 'name', entry.title);
          name.title = entry.title;
          const act = el('span', 'act');
          acts.set(entry.id, act);
          li.append(open, name, el('span', 'meta', entry.folder ? 'Folder' : metaText(entry)), act);
          return li;
        }),
      );
    }
    for (const entry of files.items) {
      if (!entry.folder) acts.get(entry.id)?.replaceChildren(...action(entry, files));
    }
    // The one opened large.
    const shown = files.items.filter((f) => !f.folder);
    const at = shown.findIndex((f) => f.id === files.viewing);
    viewer.hidden = at < 0;
    if (at >= 0) {
      const entry = shown[at];
      if (big.dataset.link !== (entry.preview ?? '')) picture(big, entry.preview, true);
      viewName.textContent = entry.title;
      viewMeta.textContent = [metaText(entry), entry.date?.replace('T', ' ').slice(0, 16) ?? ''].filter(Boolean).join(' · ');
      viewAct.replaceChildren(...action(entry, files));
      previous.disabled = at === 0;
      next.disabled = at === shown.length - 1;
      previous.onclick = () => actions.filesView(shown[at - 1].id);
      next.onclick = () => actions.filesView(shown[at + 1].id);
    }
  }

  const offFiles = watch(store, (s) => s.files, draw);
  const offDest = watch(store, (s) => s.settings, (s) => {
    where.textContent = s ? `Imports go to ${destinationText(s)}` : '';
    where.title = s?.destinationBase ?? '';
  });
  const onKey = (e: KeyboardEvent) => {
    const files = store.get().files;
    if (!files?.viewing) return;
    if (e.key === 'Escape') {
      e.stopImmediatePropagation();
      actions.filesView(null);
    } else if (e.key === 'ArrowLeft') previous.click();
    else if (e.key === 'ArrowRight') next.click();
  };
  document.addEventListener('keydown', onKey);

  return () => {
    stopped = true;
    offFiles();
    offDest();
    document.removeEventListener('keydown', onKey);
    for (const url of pictures.values()) URL.revokeObjectURL(url);
    screen.remove();
  };
}
