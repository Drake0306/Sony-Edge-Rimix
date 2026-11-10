// The quiet left column: Scopes and timer, Cameras, and Save to. The Save to
// section is also used by the App settings popover.

import type { Actions } from '../actions';
import { button, el, icon } from '../dom';
import type { AppSettings, DestinationMode } from '../ipc';
import { watch, type AppState, type AppStore, type SaveError } from '../state';
import * as intervalTimer from './intervalTimer';

const MODES: [DestinationMode, string][] = [
  ['CustomFolder', 'Custom folder'],
  ['LightroomWatched', 'Lightroom watched folder'],
  ['FolderPerDate', 'New folder per date'],
];

const today = (d = new Date()) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;

/** Where the next shot lands, with the home folder shortened to ~. */
export function destinationText(s: AppSettings): string {
  const sep = s.destinationBase.includes('\\') && !s.destinationBase.includes('/') ? '\\' : '/';
  const base = s.destinationBase.replace(/^(\/Users\/[^/]+|\/home\/[^/]+)(?=\/|$)/, '~');
  return s.destinationMode === 'FolderPerDate' ? `${base}${sep}${today()}` : base;
}

export function sect(title: string, ...children: HTMLElement[]): HTMLElement {
  const s = el('div', 'sect');
  const h = el('div', 'h');
  h.append(el('span', '', title));
  s.append(h, ...children);
  return s;
}

function group(role: string, label?: string): HTMLElement {
  const g = el('div', 'group');
  g.setAttribute('role', role);
  if (label) g.setAttribute('aria-label', label);
  return g;
}

/** A tappable settings row: `.r.row-btn > button` with key, value and a trailing mark. */
export function rowButton(key: string, value = '', trail: 'check' | 'chev' | 'none' = 'none'): HTMLElement {
  const r = el('div', 'r row-btn');
  const b = button('');
  b.append(el('span', 'k', key));
  if (value) b.append(el('span', 'v', value));
  b.append(trail === 'none' ? el('span', 'spacer14') : icon(trail === 'check' ? 'tick' : 'chr', trail === 'check' ? 'check' : 'chev', trail === 'check' ? '0 0 14 14' : '0 0 10 10'));
  r.append(b);
  return r;
}

/**
 * The Save to panel's error line. The camera layer words folder failures as
 * "Cannot save to <folder>: <error>"; anything else names the file.
 */
export function saveErrorText(e: SaveError): string {
  return e.error.startsWith('Cannot save to ') ? e.error : `Could not receive ${e.filename}: ${e.error}`;
}

/** Save to: path, Choose folder, the last save failure and the three destination modes. */
export function saveToSection(store: AppStore, actions: Actions): { el: HTMLElement; destroy: () => void } {
  const path = el('div', 'path');
  const pathText = el('span');
  const choose = button('pathbtn', 'Choose folder');
  path.append(pathText, choose);
  const failure = el('div', 'save-error');
  failure.setAttribute('role', 'alert');
  const modes = group('radiogroup', 'Destination');
  const save = (patch: Partial<AppSettings>) => {
    const s = store.get().settings;
    if (s) void actions.saveSettings({ ...s, ...patch });
  };
  choose.addEventListener('click', async () => {
    const folder = await actions.pickFolder();
    if (folder) save({ destinationBase: folder });
  });
  const offError = watch(store, (s) => s.saveError, (e) => {
    failure.hidden = e === null;
    failure.textContent = e ? saveErrorText(e) : '';
  });
  const offSettings = watch(store, (s) => s.settings, (s) => {
    pathText.textContent = s ? destinationText(s) : '';
    pathText.title = s?.destinationBase ?? '';
    modes.replaceChildren(
      ...MODES.map(([mode, label]) => {
        const on = s?.destinationMode === mode;
        const r = rowButton(label, '', on ? 'check' : 'none');
        const b = r.querySelector('button')!;
        b.setAttribute('role', 'radio');
        b.setAttribute('aria-checked', String(on));
        b.addEventListener('click', () => save({ destinationMode: mode }));
        return r;
      }),
    );
  });
  const destroy = () => {
    offError();
    offSettings();
  };
  return { el: sect('Save to', path, failure, modes), destroy };
}

function switchRow(label: string, on: boolean, toggle?: () => void): HTMLElement {
  const r = el('div', 'r');
  r.setAttribute('role', 'listitem');
  r.append(el('span', 'k', label));
  const sw = button('switch', undefined, label);
  sw.setAttribute('role', 'switch');
  sw.setAttribute('aria-checked', String(on));
  if (toggle) sw.addEventListener('click', toggle);
  else sw.title = 'Coming in the next version';
  r.append(sw);
  return r;
}

function scopes(store: AppStore, actions: Actions): { el: HTMLElement; destroy: () => void } {
  const g = group('list');
  const flip = (k: keyof AppState['overlays']) => () => {
    const o = store.get().overlays;
    store.set({ overlays: { ...o, [k]: !o[k] } });
  };
  let switches: HTMLElement[] = [];
  const offOverlays = watch(store, (s) => s.overlays, (o) => {
    switches.forEach((r) => r.remove());
    switches = [
      switchRow('Waveform', false),
      switchRow('False colour', false),
      switchRow('Zebra', o.zebra, flip('zebra')),
      switchRow('Thirds grid', o.thirds, flip('thirds')),
    ];
    g.prepend(...switches);
  });
  // The timer row stays put below the switches, which re-render on their own.
  const unmountTimer = intervalTimer.mount(g, store, actions);
  const destroy = () => {
    offOverlays();
    unmountTimer();
  };
  return { el: sect('Scopes and timer', g), destroy };
}

function cameras(store: AppStore, actions: Actions): { el: HTMLElement; destroy: () => void } {
  const g = group('list', 'Cameras');
  const key = (s: AppState) =>
    JSON.stringify([
      s.camera?.model,
      s.camera?.connected,
      s.reconnecting === null,
      s.settings?.recentCameras,
      s.settings?.lastIp,
    ]);
  const destroy = watch(store, key, (_, { camera, settings, reconnecting }) => {
    const rows: HTMLElement[] = [];
    if (camera) {
      const r = rowButton(camera.model, 'this', 'check');
      const b = r.querySelector('button')!;
      b.prepend(el('i', camera.connected ? 'dot' : 'dot off'));
      // Once the link is lost for good (a refused login, say), the row
      // connects to the camera again; not while it is still reconnecting.
      const current = settings?.recentCameras.find((c) => c.ip === settings.lastIp);
      if (!camera.connected && reconnecting === null && current) b.addEventListener('click', () => void actions.connectRecent(current));
      rows.push(r);
    }
    for (const rc of settings?.recentCameras ?? []) {
      if (camera && rc.ip === settings?.lastIp) continue;
      const r = rowButton(rc.model, rc.ssid ?? rc.ip);
      const b = r.querySelector('button')!;
      b.prepend(el('i', 'dot off'));
      b.querySelector('.k')!.classList.add('dim');
      b.addEventListener('click', () => void actions.connectRecent(rc));
      rows.push(r);
    }
    const d = rowButton('Disconnect');
    d.querySelector('button')!.addEventListener('click', () => void actions.disconnect());
    rows.push(d);
    g.replaceChildren(...rows);
  });
  return { el: sect('Cameras', g), destroy };
}

export function mount(root: HTMLElement, store: AppStore, actions: Actions): () => void {
  const col = el('aside', 'leftcol');
  col.setAttribute('aria-label', 'Tools and destination');
  const parts = [scopes(store, actions), cameras(store, actions), saveToSection(store, actions)];
  const tools = el('div', 'panel glass quiet');
  tools.append(parts[0].el, parts[1].el);
  const dest = el('div', 'panel glass quiet');
  dest.append(parts[2].el);
  col.append(tools, dest);
  root.append(col);
  return () => {
    parts.forEach((p) => p.destroy());
    col.remove();
  };
}
