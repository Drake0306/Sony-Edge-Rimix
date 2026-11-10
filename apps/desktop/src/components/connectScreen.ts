// The connect screen: scan the camera's QR code, pick a nearby camera, or
// type its Wi-Fi name and password (with the address field under Advanced),
// plus recently used cameras.

import type { Actions } from '../actions';
import { NOT_A_WIFI_CODE, webcamErrorMessage } from '../copy';
import { button, el, icon } from '../dom';
import { discover } from '../ipc';
import { parseWifiQr } from '../qr/payload';
import { createScanner, webcamSupported } from '../qr/scanner';
import { watch, type AppStore } from '../state';
import { rowButton } from './sidePanels';

export const DEFAULT_IP = '192.168.122.1';
const DISCOVER_MS = 3000;
const REFRESH_MS = 5000;
/** How long after a failed connect the webcam starts looking for a code again. */
const RESCAN_MS = 3000;
const NOTE = 'First time on this camera? Press OK on the camera screen when it asks to allow this computer.';
const SCANNING = "Hold the camera's QR code up to the webcam.";
const INFO_GLYPH =
  '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" aria-hidden="true"><circle cx="8" cy="8" r="6.5"/><path d="M8 7v4M8 5v.2" stroke-linecap="round"/></svg>';

function way(title: string, ...children: HTMLElement[]): HTMLElement {
  const a = el('article', 'way glass');
  a.append(el('h2', '', title), ...children);
  return a;
}

function input(id: string, label: string, value = ''): [HTMLLabelElement, HTMLInputElement] {
  const lbl = el('label', 'sr', label);
  lbl.htmlFor = id;
  const i = el('input');
  i.id = id;
  i.type = 'text';
  i.value = value;
  i.spellcheck = false;
  i.autocomplete = 'off';
  return [lbl, i];
}

function submitButton(): HTMLButtonElement {
  const b = button('pbtn', 'Connect');
  b.type = 'submit';
  return b;
}

export function mount(root: HTMLElement, store: AppStore, actions: Actions): () => void {
  const backdrop = el('div', 'webcam');
  backdrop.setAttribute('aria-hidden', 'true');
  backdrop.append(icon('scene-copy', '', '0 0 1600 900'));
  backdrop.firstElementChild!.setAttribute('preserveAspectRatio', 'xMidYMid slice');
  const vignette = el('div', 'vignette');

  const status = el('header', 'status glass capsule');
  status.setAttribute('aria-label', 'Status');
  status.append(el('span', 'brand', 'Rimix'), el('span', 'quietword', 'No camera connected'));

  const headline = el('div', 'headline');
  const message = el('p', 'message');
  message.setAttribute('role', 'status');
  headline.append(el('h1', '', 'Connect a camera'), el('p', '', 'Three ways in. Pick whichever is nearest to hand.'), message);

  // 1. Scan the QR code
  const video = el('video');
  video.muted = true;
  video.setAttribute('muted', '');
  video.setAttribute('playsinline', '');
  video.setAttribute('autoplay', '');
  const canvas = el('canvas');
  canvas.hidden = true;
  const preview = el('div', 'preview');
  const tag = el('span', 'tag', 'Webcam');
  tag.prepend(el('i'));
  preview.append(video, canvas, tag, el('div', 'brk'));
  const scanStatus = el('p', 'scanstatus');
  scanStatus.setAttribute('role', 'status');
  const pasteToggle = button('textbtn', "Paste the code's text");
  const pasteBox = el('div', 'field paste');
  pasteBox.hidden = true;
  const area = el('textarea');
  area.rows = 2;
  area.spellcheck = false;
  area.setAttribute('aria-label', "The code's text");
  const useText = button('pbtn', 'Use this text');
  pasteBox.append(area, useText);
  const qr = way(
    'Scan the QR code',
    preview,
    scanStatus,
    el('p', '', 'On the camera: Menu, Network, Send to Smartphone Func., Send to Smartphone.'),
    pasteToggle,
    pasteBox,
  );

  // 2. Nearby
  const nearbyList = el('div', 'devices');
  const nearby = way(
    'Nearby',
    nearbyList,
    el('p', 'fill', 'Cameras with PC Remote on appear here when this computer is on the same Wi-Fi.'),
  );

  // 3. Type the Wi-Fi name and password, with the address under Advanced
  const creds = el('form', 'creds');
  const [ssidLabel, ssid] = input('ssid', 'Wi-Fi name');
  ssid.placeholder = 'DIRECT-xxxx:ZV-E10';
  const ssidRow = el('div', 'field');
  ssidRow.append(ssidLabel, ssid);
  const [pwLabel, pw] = input('pw', 'Password');
  pw.type = 'password';
  pw.placeholder = 'Password';
  const show = button('textbtn', 'Show');
  const pwRow = el('div', 'field');
  pwRow.append(pwLabel, pw, show, submitButton());
  creds.append(ssidRow, pwRow);

  const advToggle = button('textbtn disclosure', 'Advanced: connect by address');
  advToggle.setAttribute('aria-expanded', 'false');
  const advanced = el('div', 'advanced');
  advanced.hidden = true;
  const addrForm = el('form', 'field');
  const [ipLabel, ip] = input('ip', 'Camera address', DEFAULT_IP);
  addrForm.append(ipLabel, ip, submitButton());
  advanced.append(addrForm, el('p', '', 'Shown on the camera under Network, Wi-Fi, Display Wi-Fi info.'));
  const typed = way(
    'Type the Wi-Fi name and password',
    creds,
    el('p', '', 'Shown on the camera under Menu, Network, PC Remote Function, Wi-Fi Direct Info.'),
    advToggle,
    advanced,
  );

  const ways = el('div', 'ways');
  ways.append(qr, nearby, typed);

  const note = el('p', 'note glass quiet');
  note.innerHTML = INFO_GLYPH;
  note.append(NOTE);
  const recent = el('div', 'recent glass quiet');
  const recentGroup = el('div', 'group');
  recent.append(el('div', 'h', 'Recently used'), recentGroup);
  const foot = el('div', 'foot');
  foot.append(note, recent);

  const parts = [backdrop, vignette, status, headline, ways, foot];
  root.append(...parts);

  // Every Connect button, Use this text and the recent rows wait while a connect runs.
  const applyBusy = () => {
    const busy = store.get().connecting !== null;
    root.querySelectorAll<HTMLButtonElement>('.pbtn, .recent button').forEach((b) => (b.disabled = busy));
  };

  let stopped = false;
  let rescanTimer = 0;
  const scanner = createScanner({
    deviceId: store.get().settings?.webcamId,
    onResult: (text) => void useCode(text, true),
  });
  const startScanner = () => {
    scanStatus.textContent = SCANNING;
    scanner.start(video, canvas).catch((e) => {
      if (!stopped) scanStatus.textContent = webcamErrorMessage(e);
    });
  };

  const showPaste = () => {
    pasteBox.hidden = false;
  };

  const rescan = () => {
    if (!stopped) rescanTimer = window.setTimeout(startScanner, RESCAN_MS);
  };

  // The webcam may read another QR code first (Sony's app link, say); the
  // name field takes focus only the first time, so typing is not interrupted.
  let focusedForCode = false;

  /** Connects with a code's text, or explains why it is not a Wi-Fi code. */
  async function useCode(text: string, fromScanner: boolean): Promise<void> {
    const wifi = parseWifiQr(text);
    if (!wifi) {
      store.set({ error: NOT_A_WIFI_CODE });
      showPaste();
      if (!fromScanner) {
        ssid.focus();
        return;
      }
      if (!area.value) area.value = text;
      if (!focusedForCode) ssid.focus();
      focusedForCode = true;
      rescan();
      return;
    }
    const ok = store.get().connecting === null && (await actions.connectWithWifi(wifi));
    if (!ok && fromScanner) rescan();
  }

  pasteToggle.addEventListener('click', () => {
    showPaste();
    area.focus();
  });
  useText.addEventListener('click', () => void useCode(area.value, false));
  show.addEventListener('click', () => {
    const hidden = pw.type === 'password';
    pw.type = hidden ? 'text' : 'password';
    show.textContent = hidden ? 'Hide' : 'Show';
  });
  creds.addEventListener('submit', (e) => {
    e.preventDefault();
    const name = ssid.value.trim();
    if (!name) {
      ssid.focus();
      return;
    }
    void actions.connectWithWifi({ ssid: name, password: pw.value });
  });
  advToggle.addEventListener('click', () => {
    advanced.hidden = !advanced.hidden;
    advToggle.setAttribute('aria-expanded', String(!advanced.hidden));
    if (!advanced.hidden) ip.focus();
  });
  addrForm.addEventListener('submit', (e) => {
    e.preventDefault();
    if (ip.value.trim()) void actions.connect(ip.value);
  });

  const offs = [
    // Progress while connecting, otherwise the last error.
    watch(store, (s) => s.connecting ?? s.error, (_, s) => {
      message.textContent = s.connecting?.text ?? s.error ?? '';
      message.classList.toggle('error', !s.connecting && !!s.error);
      applyBusy();
    }),
    watch(store, (s) => s.nearby, (list) => {
      if (list.length === 0) {
        nearbyList.replaceChildren(el('p', 'looking', 'Looking for cameras on this network'));
        return;
      }
      nearbyList.replaceChildren(
        ...list.map((cam) => {
          const d = el('div', 'device');
          const t = el('div', 't');
          t.append(el('b', '', cam.model || cam.friendly_name), el('span', '', cam.ip));
          const b = button('pbtn', 'Connect', `Connect to ${cam.model} at ${cam.ip}`);
          b.addEventListener('click', () => void actions.connect(cam.ip));
          d.append(t, b);
          return d;
        }),
      );
      applyBusy();
    }),
    watch(store, (s) => s.settings?.recentCameras, (list = []) => {
      recent.hidden = list.length === 0;
      recentGroup.replaceChildren(
        ...list.map((rc) => {
          const r = rowButton(rc.model, rc.ssid ?? rc.ip, 'chev');
          const b = r.querySelector('button')!;
          b.prepend(icon('cam', 'camglyph', '0 0 20 20'));
          b.addEventListener('click', () => void actions.connectRecent(rc));
          return r;
        }),
      );
      applyBusy();
    }),
  ];

  if (webcamSupported()) startScanner();
  else scanStatus.textContent = webcamErrorMessage(new Error('unsupported'));

  let timer = 0;
  const scan = async () => {
    const started = Date.now();
    try {
      const found = await discover(DISCOVER_MS);
      if (!stopped) store.set({ nearby: found });
    } catch {
      // Discovery is best effort; the other ways always work.
    }
    if (!stopped) timer = window.setTimeout(scan, Math.max(0, REFRESH_MS - (Date.now() - started)));
  };
  void scan();

  return () => {
    stopped = true;
    window.clearTimeout(timer);
    window.clearTimeout(rescanTimer);
    scanner.stop();
    offs.forEach((off) => off());
    parts.forEach((p) => p.remove());
  };
}
