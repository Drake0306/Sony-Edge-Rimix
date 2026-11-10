// The App settings button and popover: Appearance and Layout as segmented
// controls, then Save to, then the shortcut hint. These settings are about
// Rimix itself; the camera's settings live in their own popover.

import type { Actions } from '../actions';
import { button, el, icon } from '../dom';
import { openLogs, type AppSettings, type Appearance, type Layout, type RecentCamera, type WifiJoin } from '../ipc';
import { listWebcams } from '../qr/scanner';
import { defaultSettings, watch, type AppStore } from '../state';
import { applyAppearance, applyLayout } from '../theme';
import { saveToSection, sect } from './sidePanels';

const KEYBOARD_GLYPH =
  '<svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" aria-hidden="true"><rect x="1" y="2.5" width="10" height="7" rx="1.5"/><path d="M3.5 7.5h5" stroke-linecap="round"/></svg>';

const isMac = (platform: string) => /mac|iphone|ipad/i.test(platform);

export function shortcutHint(platform: string = navigator.platform): string {
  return `Shortcut: ${isMac(platform) ? '⌘' : 'Ctrl+'}, opens settings`;
}

interface Segmented<T extends string> {
  el: HTMLElement;
  sync(value: T | undefined): void;
}

/** One settings row holding a segmented control (the mockup's `.mini`). */
function segmented<T extends string>(
  key: string,
  label: string,
  values: T[],
  choose: (value: T) => void,
  labels: string[] = values,
): Segmented<T> {
  const r = el('div', 'r');
  r.setAttribute('role', 'listitem');
  const mini = el('div', 'mini');
  mini.setAttribute('role', 'radiogroup');
  mini.setAttribute('aria-label', label);
  const buttons = values.map((v) => {
    const b = button('', labels[values.indexOf(v)]);
    b.setAttribute('role', 'radio');
    b.addEventListener('click', () => choose(v));
    mini.append(b);
    return b;
  });
  r.append(el('span', 'k', key), mini);
  const g = el('div', 'group');
  g.setAttribute('role', 'list');
  g.append(r);
  return {
    el: g,
    sync(value) {
      buttons.forEach((b, i) => {
        const on = values[i] === value;
        b.classList.toggle('on', on);
        b.setAttribute('aria-checked', String(on));
      });
    },
  };
}

const where = (rc: RecentCamera) => rc.ssid ?? rc.ip;

export function mount(root: HTMLElement, store: AppStore, actions: Actions): () => void {
  const gear = button('gear glass quiet', undefined, 'App settings');
  gear.setAttribute('aria-haspopup', 'dialog');
  gear.setAttribute('aria-expanded', 'false');
  gear.setAttribute('aria-controls', 'apppop');
  gear.append(icon('gearglyph', '', '0 0 20 20'), el('span', 'lbl', 'App settings'));

  const arrow = el('div', 'aparrow');
  arrow.setAttribute('aria-hidden', 'true');
  const pop = el('div', 'apppop glass');
  pop.id = 'apppop';
  pop.setAttribute('role', 'dialog');
  pop.setAttribute('aria-label', 'App settings');

  /** Applies at once, then saves on top of the latest settings so recents and Save to survive. */
  const change = (patch: Partial<AppSettings>) => {
    if (patch.appearance) applyAppearance(patch.appearance);
    if (patch.layout) applyLayout(patch.layout);
    const next = { ...(store.get().settings ?? defaultSettings()), ...patch };
    store.set({ settings: next });
    void actions.saveSettings(next);
  };
  const theme = segmented<Appearance>('Theme', 'Theme', ['Dark', 'Light', 'System'], (v) =>
    change({ appearance: v }),
  );
  const layout = segmented<Layout>('Controls', 'Controls layout', ['Dock', 'Ring'], (v) => change({ layout: v }));
  const wifi = segmented<WifiJoin>('Wi-Fi join', 'Wi-Fi join', ['Automatic', 'Ask'], (v) => change({ wifiJoin: v }), [
    'Automatic',
    'Ask me',
  ]);

  const select = document.createElement('select');
  select.className = 'webcam-select';
  select.setAttribute('aria-label', 'Webcam');
  const webcamRow = el('div', 'r');
  webcamRow.append(el('span', 'k', 'Webcam'), select);
  const webcamGroup = el('div', 'group');
  webcamGroup.setAttribute('role', 'list');
  webcamRow.setAttribute('role', 'listitem');
  webcamGroup.append(webcamRow);
  let cams: { id: string; label: string }[] = [];
  const fillWebcams = () => {
    const opts = [new Option('Default', ''), ...cams.map((c) => new Option(c.label, c.id))];
    select.replaceChildren(...opts);
    const id = store.get().settings?.webcamId ?? '';
    select.value = cams.some((c) => c.id === id) ? id : '';
  };
  select.addEventListener('change', () => change({ webcamId: select.value || null }));
  fillWebcams();

  const remembered = el('div', 'group remembered');
  remembered.setAttribute('role', 'list');
  remembered.setAttribute('aria-label', 'Remembered cameras');
  const fillRemembered = (list: RecentCamera[]) => {
    if (list.length === 0) {
      remembered.replaceChildren(el('div', 'quiet', 'None yet'));
      return;
    }
    remembered.replaceChildren(
      ...list.map((rc) => {
        const r = el('div', 'r');
        r.setAttribute('role', 'listitem');
        const forget = button('pathbtn', 'Forget', `Forget ${rc.model} on ${where(rc)}`);
        forget.addEventListener('click', () => void actions.forgetCamera(rc));
        r.append(el('span', 'k', rc.model), el('span', 'v', where(rc)), forget);
        return r;
      }),
    );
  };
  const logsGroup = el('div', 'group');
  logsGroup.setAttribute('role', 'list');
  const logsRow = el('div', 'r');
  logsRow.setAttribute('role', 'listitem');
  const showLogs = button('pathbtn', 'Show log folder');
  showLogs.addEventListener('click', () =>
    void openLogs().catch((e) => actions.toast(`Could not open the log folder: ${e instanceof Error ? e.message : String(e)}`)),
  );
  logsRow.append(el('span', 'k', 'Logs'), showLogs);
  logsGroup.append(logsRow);

  const saveTo = saveToSection(store, actions);
  const hint = el('div', 'hint');
  hint.innerHTML = KEYBOARD_GLYPH;
  hint.append(shortcutHint());
  pop.append(el('div', 'ttl', 'App settings'), sect('Appearance', theme.el), sect('Layout', layout.el),
    sect('Connection', wifi.el, webcamGroup, logsGroup, el('div', 'sub', 'Remembered cameras'), remembered),
    saveTo.el, hint);
  pop.hidden = arrow.hidden = true;

  const setOpen = (open: boolean) => {
    pop.hidden = arrow.hidden = !open;
    gear.setAttribute('aria-expanded', String(open));
    if (open) {
      void listWebcams().then((list) => {
        cams = list;
        fillWebcams();
      });
    }
    if (open) pop.querySelector<HTMLElement>('.mini .on, button')?.focus();
  };
  gear.addEventListener('click', () => setOpen(pop.hidden));
  const onKey = (e: KeyboardEvent) => {
    if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key === ',') {
      e.preventDefault();
      if (!e.repeat) setOpen(pop.hidden);
    } else if (e.key === 'Escape' && !pop.hidden) {
      // Close only this popover; a camera settings popover under it stays.
      e.stopImmediatePropagation();
      setOpen(false);
      gear.focus();
    }
  };
  document.addEventListener('keydown', onKey);
  root.append(gear, arrow, pop);

  const off = watch(store, (s) => s.settings, (s) => {
    theme.sync(s?.appearance);
    layout.sync(s?.layout);
    wifi.sync(s?.wifiJoin);
  });
  const offRecent = watch(store, (s) => s.settings?.recentCameras, (list) => fillRemembered(list ?? []));

  return () => {
    off();
    offRecent();
    saveTo.destroy();
    document.removeEventListener('keydown', onKey);
    [gear, arrow, pop].forEach((e) => e.remove());
  };
}
