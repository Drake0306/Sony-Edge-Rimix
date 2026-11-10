import { describe, expect, test, vi } from 'vitest';
import cameraState from '../fixtures/camera-state.json';
import cardObject from '../fixtures/card-object.json';
import type { Actions } from '../actions';
import type { CameraState, CardObject } from '../ipc';
import { createStore, emptyCard, initialState, type AppState, type CardState } from '../state';
import { dateText, mount, sizeText, sortCard, TRANSFER_MODE_TEXT } from './cardBrowser';

const camera = { ...(cameraState as unknown as CameraState), model: 'ZV-E10' };
const ID = '10.0.0.5:15740';
const jpeg: CardObject = { ...(cardObject as CardObject), captured: '2026-11-10T10:15:00' };
const raw: CardObject = { ...jpeg, handle: 258, filename: 'DSC00010.ARW', bytes: 24 << 20, format: 0xb101, kind: 'Raw' };
const clip: CardObject = {
  ...jpeg, handle: 259, filename: 'C0001.MP4', bytes: 64 << 20, format: 0xb982, kind: 'Video', captured: '2026-11-10T10:20:00',
};

function setup(card: Partial<CardState> = {}, open = true) {
  const store = createStore<AppState>({
    ...initialState(),
    screen: 'main',
    camera,
    cameras: { [ID]: camera },
    activeId: ID,
    cardOpen: open,
    card: { ...emptyCard(), mode: 'same-session', items: [jpeg, raw, clip], ...card },
  });
  const actions = { cardRefresh: vi.fn(() => Promise.resolve()), cardDownload: vi.fn(() => Promise.resolve()) };
  const root = document.createElement('div');
  document.body.append(root);
  const unmount = mount(root, store, actions as unknown as Actions);
  const panel = () => root.querySelector<HTMLElement>('.cardpanel')!;
  const rows = () => [...root.querySelectorAll<HTMLElement>('.card-row')];
  const row = (handle: number) => root.querySelector<HTMLElement>(`.card-row[data-handle="${handle}"]`)!;
  const body = () => root.querySelector<HTMLElement>('.cardpanel .card-body')!;
  const done = () => {
    unmount();
    root.remove();
  };
  return { store, actions, root, panel, rows, row, body, done };
}

const buttonIn = (el: HTMLElement, text: string) =>
  [...el.querySelectorAll('button')].find((b) => b.textContent === text) ?? null;

describe('formatting', () => {
  test('sizes read in binary units with one decimal under 10', () => {
    expect(sizeText(2 << 20)).toBe('2.0 MB');
    expect(sizeText(64 << 20)).toBe('64 MB');
    expect(sizeText(Math.round(1.2 * 2 ** 30))).toBe('1.2 GB');
    expect(sizeText(512)).toBe('512 B');
  });

  test('dates drop the seconds and the T', () => {
    expect(dateText('2026-11-11T14:03:00')).toBe('2026-11-11 14:03');
    expect(dateText(null)).toBe('');
  });

  test('newest first, then by handle', () => {
    const undated = { ...jpeg, handle: 300, captured: null };
    expect(sortCard([jpeg, undated, raw, clip]).map((o) => o.handle)).toEqual([259, 258, 257, 300]);
  });
});

describe('card browser', () => {
  test('stays hidden until opened, then lists the card from a refresh', () => {
    const t = setup({}, false);
    expect(t.panel().hidden).toBe(true);
    expect(t.actions.cardRefresh).not.toHaveBeenCalled();
    t.store.set({ cardOpen: true });
    expect(t.panel().hidden).toBe(false);
    expect(t.actions.cardRefresh).toHaveBeenCalledTimes(1);
    expect(t.panel().getAttribute('role')).toBe('dialog');
    expect(t.panel().querySelector('h2')!.textContent).toBe('Browse card · ZV-E10');
    t.done();
  });

  test('renders three rows newest first and Download asks for that handle', () => {
    const t = setup();
    expect(t.rows().map((r) => r.querySelector('.name')!.textContent)).toEqual([
      'C0001.MP4', 'DSC00010.ARW', 'DSC00010.JPG',
    ]);
    const first = t.rows()[0];
    expect(first.querySelector('.kind')!.getAttribute('aria-label')).toBe('Video');
    expect(first.querySelector('.size')!.textContent).toBe('64 MB');
    expect(first.querySelector('.date')!.textContent).toBe('2026-11-10 10:20');
    expect(t.row(257).querySelector('.size')!.textContent).toBe('2.0 MB');
    expect(t.row(258).querySelector('.kind')!.getAttribute('aria-label')).toBe('Raw');
    buttonIn(t.row(258), 'Download')!.click();
    expect(t.actions.cardDownload).toHaveBeenCalledWith(258);
    t.done();
  });

  test('progress updates the row, then Saved replaces it', () => {
    const t = setup();
    t.store.set({ card: { ...t.store.get().card, downloads: { 257: { received: 1 << 20, total: 2 << 20 } } } });
    const bar = t.row(257).querySelector<HTMLElement>('[role="progressbar"]')!;
    expect(bar.getAttribute('aria-valuenow')).toBe('50');
    expect(t.row(257).querySelector('.ptext')!.textContent).toBe('1.0 MB/2.0 MB');
    expect(buttonIn(t.row(257), 'Download')).toBeNull();
    // The other rows keep their button.
    expect(buttonIn(t.row(258), 'Download')).not.toBeNull();
    t.store.set({ card: { ...t.store.get().card, downloads: {}, done: { 257: '/shots/DSC00010.JPG' } } });
    expect(t.row(257).querySelector('[role="progressbar"]')).toBeNull();
    expect(t.row(257).querySelector('.saved')!.textContent).toBe('Saved');
    expect(buttonIn(t.row(257), 'Download')).toBeNull();
    t.done();
  });

  test('a file of 4 GiB or more cannot be downloaded', () => {
    const t = setup({ items: [{ ...clip, size_known: false, bytes: 0xffffffff }] });
    expect(t.row(259).querySelector('.toolarge')!.textContent).toBe('Too large to download');
    expect(t.row(259).querySelector('button')).toBeNull();
    t.done();
  });

  test('a failed download shows its error on the row and can be tried again', () => {
    const t = setup({ failed: { 259: 'timed out' } });
    expect(t.row(259).querySelector('.row-error')!.textContent).toBe('timed out');
    expect(buttonIn(t.row(259), 'Download')).not.toBeNull();
    expect(t.row(258).querySelector('.row-error')).toBeNull();
    t.done();
  });

  test('says when the card is empty or still loading', () => {
    const t = setup({ items: [], loading: true });
    expect(t.body().textContent).toBe('Loading…');
    t.store.set({ card: { ...t.store.get().card, loading: false } });
    expect(t.body().textContent).toBe('No files on the card');
    t.store.set({ card: { ...t.store.get().card, error: 'Not connected to a camera' } });
    expect(t.body().textContent).toBe('Not connected to a camera');
    t.done();
  });

  test('a camera that needs its transfer mode explains instead of listing', () => {
    const t = setup({ mode: 'needs-transfer-mode', items: [] });
    expect(t.body().textContent).toBe(TRANSFER_MODE_TEXT);
    expect(TRANSFER_MODE_TEXT).toBe('This camera needs its own transfer mode for card browsing; not available yet');
    expect(t.rows()).toEqual([]);
    t.done();
  });

  test('Refresh lists again; Close and Escape close', () => {
    const t = setup();
    t.actions.cardRefresh.mockClear();
    buttonIn(t.panel(), 'Refresh')!.click();
    expect(t.actions.cardRefresh).toHaveBeenCalledTimes(1);
    buttonIn(t.panel(), 'Close')!.click();
    expect(t.store.get().cardOpen).toBe(false);
    expect(t.panel().hidden).toBe(true);
    t.store.set({ cardOpen: true });
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(t.store.get().cardOpen).toBe(false);
    // Closed, Escape is left to others.
    const seen = vi.fn();
    document.addEventListener('keydown', seen);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(seen).toHaveBeenCalled();
    document.removeEventListener('keydown', seen);
    t.done();
  });
});
