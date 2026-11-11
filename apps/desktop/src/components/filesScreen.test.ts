import { beforeEach, describe, expect, test, vi } from 'vitest';
import type { Actions } from '../actions';
import type { FileEntry } from '../ipc';
import { createStore, defaultSettings, initialState, type AppState, type FilesState } from '../state';
import { kindOf, metaText, mount, NOTHING_CHOSEN } from './filesScreen';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => Promise.resolve(new ArrayBuffer(4))) }));

const file = (id: string, title: string, over: Partial<FileEntry> = {}): FileEntry => ({
  id, title, folder: false, date: '2026-11-11T15:28:10', thumb: `http://c/TN_${title}`, preview: `http://c/LRG_${title}`,
  file: `http://c/ORG_${title}`, bytes: 6_100_000, resolution: '6000x4000', original: true, ...over,
});
const items = [
  file('a', 'DSC00412.JPG'),
  file('b', 'DSC00413.ARW', { file: 'http://c/LRG_DSC00413.ARW', bytes: null, resolution: null, original: false }),
  file('c', 'C0007.MP4', { bytes: 912_000_000, resolution: null }),
];
const files = (over: Partial<FilesState> = {}): FilesState => ({
  info: { model: 'ZV-E10', root: 'PushRoot', address: '192.168.122.1:64321' },
  path: [{ id: null, title: 'ZV-E10' }, { id: 'd1', title: '2026-11-11' }],
  items, loading: false, error: null, progress: {}, saved: {}, failed: {}, viewing: null, ...over,
});

function setup(state: Partial<FilesState> = {}) {
  URL.createObjectURL = vi.fn(() => 'blob:x');
  URL.revokeObjectURL = vi.fn();
  const store = createStore<AppState>({ ...initialState(), screen: 'files', settings: { ...defaultSettings('/Users/you/Pictures/Rimix'), destinationMode: 'CustomFolder' }, files: files(state) });
  const actions = {
    filesEnter: vi.fn(() => Promise.resolve()),
    filesImport: vi.fn(() => Promise.resolve()),
    filesImportAll: vi.fn(() => Promise.resolve()),
    filesView: vi.fn((id: string | null) => store.set({ files: { ...store.get().files!, viewing: id } })),
    filesDone: vi.fn(() => Promise.resolve()),
    pickFolder: vi.fn(() => Promise.resolve(null)),
    saveSettings: vi.fn(() => Promise.resolve()),
  };
  const root = document.createElement('div');
  document.body.append(root);
  const unmount = mount(root, store, actions as unknown as Actions);
  const tile = (id: string) => root.querySelector<HTMLElement>(`.tile[data-id="${id}"]`)!;
  return { root, store, actions, tile, done: () => { unmount(); root.remove(); } };
}

describe('files screen', () => {
  beforeEach(() => vi.clearAllMocks());

  test('says whose files, what is listed, where it is and where imports go', () => {
    const t = setup();
    expect(t.root.querySelector('.files-head h1')!.textContent).toBe('Files on ZV-E10');
    expect(t.root.querySelector('.files-head p')!.textContent).toBe('The pictures chosen on the camera');
    expect([...t.root.querySelectorAll('.files-path button')].map((b) => b.textContent)).toEqual(['ZV-E10', '2026-11-11']);
    expect(t.root.querySelector('.files-bar .where')!.textContent).toBe('Imports go to ~/Pictures/Rimix');
    t.store.set({ files: files({ info: { model: 'ILCE-7M3', root: 'PhotoRoot', address: 'x' } }) });
    expect(t.root.querySelector('.files-head p')!.textContent).toBe('Everything on the card');
    t.done();
  });

  test('every file has a small picture, its name and what is known of it, and an Import button', async () => {
    const t = setup();
    expect([...t.root.querySelectorAll('.tile .name')].map((n) => n.textContent)).toEqual(['DSC00412.JPG', 'DSC00413.ARW', 'C0007.MP4']);
    expect(t.tile('a').querySelector('.meta')!.textContent).toBe('5.8 MB · 6000x4000');
    // A RAW the camera only sends a JPEG copy of says so.
    expect(t.tile('b').querySelector('.meta')!.textContent).toBe('JPEG copy');
    expect(t.tile('b').querySelector('.kind')!.textContent).toBe('RAW');
    expect(t.tile('c').querySelector('.kind')!.textContent).toBe('Movie');
    // The small pictures come from the camera a few at a time.
    await new Promise((r) => setTimeout(r, 0));
    await new Promise((r) => setTimeout(r, 0));
    expect(t.tile('a').querySelector<HTMLImageElement>('img')!.getAttribute('src')).toBe('blob:x');
    t.tile('a').querySelector<HTMLButtonElement>('.act button')!.click();
    expect(t.actions.filesImport).toHaveBeenCalledWith(items[0]);
    // Importing is not looking: the picture did not open.
    expect(t.actions.filesView).not.toHaveBeenCalled();
    t.done();
  });

  test('an import shows how far it is, then that it is imported; a failure says why and can be tried again', () => {
    const t = setup({ progress: { a: { received: 3_050_000, total: 6_100_000 } }, saved: { c: '/shots/C0007.MP4' }, failed: { b: 'answered 406' } });
    expect(t.tile('a').querySelector('[role="progressbar"]')!.getAttribute('aria-valuenow')).toBe('50');
    expect(t.tile('a').querySelector('.act button')).toBeNull();
    expect(t.tile('c').querySelector('.saved')!.textContent).toBe('Imported');
    expect(t.tile('b').querySelector('.failed')!.textContent).toBe('answered 406');
    expect(t.tile('b').querySelector('.act button')).not.toBeNull();
    // Nothing more is started while one is under way.
    expect([...t.root.querySelectorAll<HTMLButtonElement>('.files-bar button')].find((b) => b.textContent === 'Import all')!.disabled).toBe(true);
    t.done();
  });

  test('a file opens large to look at, with the next and the one before; Escape closes it', () => {
    const t = setup();
    const viewer = t.root.querySelector<HTMLElement>('.files-view')!;
    expect(viewer.hidden).toBe(true);
    t.tile('a').querySelector<HTMLButtonElement>('.pic')!.click();
    expect(viewer.hidden).toBe(false);
    expect(viewer.querySelector('b')!.textContent).toBe('DSC00412.JPG');
    expect(viewer.querySelector('.meta')!.textContent).toBe('5.8 MB · 6000x4000 · 2026-11-11 15:28');
    const [previous, next] = ['Previous', 'Next'].map((n) => [...viewer.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent === n)!);
    expect(previous.disabled).toBe(true);
    next.click();
    expect(viewer.querySelector('b')!.textContent).toBe('DSC00413.ARW');
    viewer.querySelector<HTMLButtonElement>('.act button')!.click();
    expect(t.actions.filesImport).toHaveBeenCalledWith(items[1]);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(viewer.hidden).toBe(true);
    t.done();
  });

  test('folders are entered, the path goes back, and Done ends it', () => {
    const folder = file('d1', '2026-11-11', { folder: true, thumb: null, preview: null, file: null, bytes: null, resolution: null });
    const t = setup({ items: [folder], path: [{ id: null, title: 'ZV-E10' }] });
    t.tile('d1').querySelector<HTMLButtonElement>('.pic')!.click();
    expect(t.actions.filesEnter).toHaveBeenCalledWith(folder);
    t.store.set({ files: files() });
    t.root.querySelector<HTMLButtonElement>('.files-path button')!.click();
    expect(t.actions.filesEnter).toHaveBeenLastCalledWith(0);
    [...t.root.querySelectorAll<HTMLButtonElement>('.files-bar button')].find((b) => b.textContent === 'Done')!.click();
    expect(t.actions.filesDone).toHaveBeenCalled();
    t.done();
  });

  test('an empty listing says what to do on the camera; an error is shown', () => {
    const t = setup({ items: [] });
    expect(t.root.querySelector('.files-note')!.textContent).toBe(NOTHING_CHOSEN);
    t.store.set({ files: files({ items: [], error: 'the camera would not list PushRoot (answered 500)' }) });
    expect(t.root.querySelector('.files-note')!.textContent).toContain('answered 500');
    expect(t.root.querySelector('.files-note')!.classList.contains('error')).toBe(true);
    t.done();
  });

  test('kinds and what is known of a file', () => {
    expect([kindOf('DSC1.JPG'), kindOf('DSC1.ARW'), kindOf('C1.MP4'), kindOf('x.bin')]).toEqual(['picture', 'raw', 'movie', 'file']);
    expect(metaText(items[2])).toBe('870 MB');
  });
});
