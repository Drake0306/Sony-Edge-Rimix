// The App settings button and popover: Appearance and Layout as segmented
// controls, Connection (with the Bluetooth switch), FTP transfer from the camera, then Save to, then the
// shortcut hint. These settings are about Rimix itself; the camera's
// settings live in their own popover.

import type { Actions } from '../actions';
import { FTP_NAME, FTP_WHAT } from '../copy';
import { button, el, icon } from '../dom';
import {
  bleRetry,
  openLogs,
  type AppSettings,
  type Appearance,
  type Layout,
  type RecentCamera,
  type SftpStatus,
  type WifiJoin,
} from '../ipc';
import { listWebcams } from '../qr/scanner';
import { defaultSettings, watch, type AppStore } from '../state';
import { applyAppearance, applyLayout } from '../theme';
import { saveToSection, sect, switchRow } from './sidePanels';

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

export const SFTP_HINT =
  'On the camera: Menu, Network, FTP Transfer Func., Server Setting; choose SFTP, enter this address, username and password.';

/** Where the camera sends to: the LAN address, or a stand-in with the port when it is unknown. */
export const sftpAddress = (st: SftpStatus) => st.address ?? `this computer's address:${st.port}`;

function infoRow(key: string, value: string, valueClass = 'v'): HTMLElement {
  const r = el('div', 'r');
  r.setAttribute('role', 'listitem');
  r.append(el('span', 'k', key), el('span', valueClass, value));
  return r;
}

/**
 * FTP transfer from the camera: a switch, and while it is on, what to type into the
 * camera's FTP Transfer server settings. `refresh` re-reads the status
 * (the popover calls it when it opens); `reset` hides the password again.
 */
function receiveSection(store: AppStore, actions: Actions) {
  const toggle = button('switch', undefined, FTP_NAME);
  toggle.setAttribute('role', 'switch');
  toggle.append(el('i'));
  toggle.addEventListener('click', () => void actions.setSftpEnabled(store.get().sftp?.enabled !== true));
  const switchRow = el('div', 'r');
  switchRow.setAttribute('role', 'listitem');
  switchRow.append(el('span', 'k', 'Receive files by FTP'), toggle);
  const rows = el('div', 'group sftp');
  rows.setAttribute('role', 'list');
  rows.append(switchRow);
  const details = el('div', 'sftp-details');
  const error = el('div', 'sftp-error');
  error.setAttribute('role', 'status');
  let revealed = false;

  // The password row is kept, so Show keeps its place while it toggles.
  const password = el('div', 'r');
  password.setAttribute('role', 'listitem');
  const secret = el('span', 'v');
  const show = button('pathbtn');
  password.append(el('span', 'k', 'Password'), secret, show);
  const paint = () => {
    const word = store.get().sftp?.password ?? '';
    secret.textContent = revealed ? word : '•'.repeat(word.length);
    show.textContent = revealed ? 'Hide' : 'Show';
    show.setAttribute('aria-label', revealed ? 'Hide password' : 'Show password');
  };
  show.addEventListener('click', () => {
    revealed = !revealed;
    paint();
  });
  const fresh = button('pathbtn', 'New password');
  fresh.addEventListener('click', () => void actions.regenerateSftpPassword());
  const freshRow = el('div', 'r');
  freshRow.setAttribute('role', 'listitem');
  freshRow.append(el('span', 'k', ''), fresh);

  const render = (st: SftpStatus | null) => {
    const on = st?.enabled === true;
    toggle.setAttribute('aria-checked', String(on));
    toggle.classList.toggle('on', on);
    error.textContent = st?.error ?? '';
    error.hidden = !st?.error;
    paint();
    if (!st || !on) {
      rows.replaceChildren(switchRow);
      details.replaceChildren();
      return;
    }
    rows.replaceChildren(
      switchRow,
      infoRow('Address', sftpAddress(st)),
      infoRow('Username', st.user),
      password,
      ...(st.fingerprint ? [infoRow('Fingerprint', st.fingerprint, 'v fp')] : []),
      freshRow,
    );
    details.replaceChildren(el('div', 'sftp-hint', SFTP_HINT));
  };
  const off = watch(store, (s) => s.sftp, render);
  return {
    el: sect(FTP_NAME, el('p', 'sftp-what', FTP_WHAT), rows, error, details),
    refresh: () => void actions.refreshSftp(),
    reset: () => {
      revealed = false;
      paint();
    },
    destroy: off,
  };
}

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
  // Bluetooth: turning it on tries the radio again first, so a scan right
  // after does not get the unavailable answer remembered from before.
  const bluetoothGroup = el('div', 'group');
  bluetoothGroup.setAttribute('role', 'list');
  const setBluetooth = (on: boolean) => {
    if (!on) {
      change({ bluetooth: false });
      return;
    }
    void bleRetry()
      .catch(() => {})
      .then(() => change({ bluetooth: true }));
  };
  const bluetoothOn = () => store.get().settings?.bluetooth ?? true;
  const bluetoothRow = switchRow('Bluetooth', bluetoothOn(), () => setBluetooth(!bluetoothOn()));
  bluetoothGroup.append(bluetoothRow);
  const bluetoothSwitch = bluetoothRow.querySelector('[role="switch"]')!;

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

  const receive = receiveSection(store, actions);
  const saveTo = saveToSection(store, actions);
  const hint = el('div', 'hint');
  hint.innerHTML = KEYBOARD_GLYPH;
  hint.append(shortcutHint());
  pop.append(el('div', 'ttl', 'App settings'), sect('Appearance', theme.el), sect('Layout', layout.el),
    sect('Connection', wifi.el, bluetoothGroup, webcamGroup, logsGroup, el('div', 'sub', 'Remembered cameras'), remembered),
    receive.el, saveTo.el, hint);
  pop.hidden = arrow.hidden = true;

  const setOpen = (open: boolean) => {
    pop.hidden = arrow.hidden = !open;
    gear.setAttribute('aria-expanded', String(open));
    if (open) {
      void listWebcams().then((list) => {
        cams = list;
        fillWebcams();
      });
      receive.refresh();
    } else {
      receive.reset();
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
    bluetoothSwitch.setAttribute('aria-checked', String(s?.bluetooth ?? true));
  });
  const offRecent = watch(store, (s) => s.settings?.recentCameras, (list) => fillRemembered(list ?? []));

  return () => {
    off();
    offRecent();
    saveTo.destroy();
    receive.destroy();
    document.removeEventListener('keydown', onKey);
    [gear, arrow, pop].forEach((e) => e.remove());
  };
}
