// The connect screen: nearby cameras, an address field, recently used
// cameras, and a QR panel that arrives in the next version.

import type { Actions } from '../actions';
import { button, el, icon } from '../dom';
import { discover } from '../ipc';
import { watch, type AppStore } from '../state';
import { rowButton } from './sidePanels';

export const DEFAULT_IP = '192.168.122.1';
const DISCOVER_MS = 3000;
const REFRESH_MS = 5000;
const NOTE = 'First time on this camera? Press OK on the camera screen when it asks to allow this computer.';
const INFO_GLYPH =
  '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" aria-hidden="true"><circle cx="8" cy="8" r="6.5"/><path d="M8 7v4M8 5v.2" stroke-linecap="round"/></svg>';

function way(title: string, ...children: HTMLElement[]): HTMLElement {
  const a = el('article', 'way glass');
  a.append(el('h2', '', title), ...children);
  return a;
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

  const preview = el('div', 'preview');
  const tag = el('span', 'tag', 'Webcam');
  tag.prepend(el('i'));
  preview.append(tag, el('div', 'brk'));
  const qr = way('Scan the QR code', preview, el('p', '', 'Coming in the next version'));

  const nearbyList = el('div', 'devices');
  const nearby = way(
    'Nearby',
    nearbyList,
    el('p', 'fill', 'Cameras with PC Remote on appear here when this computer is on the same Wi-Fi.'),
  );

  const form = el('form', 'field');
  const lbl = el('label', 'sr', 'Camera address');
  lbl.htmlFor = 'ip';
  const input = el('input');
  input.id = 'ip';
  input.type = 'text';
  input.value = DEFAULT_IP;
  input.spellcheck = false;
  const submit = button('pbtn', 'Connect');
  submit.type = 'submit';
  form.append(lbl, input, submit);
  const address = way("Enter the camera's address", form, el('p', '', 'Shown on the camera under Network, Wi-Fi, Display Wi-Fi info.'));

  const ways = el('div', 'ways');
  ways.append(qr, nearby, address);

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

  let connecting = false;
  async function connectTo(ip: string): Promise<void> {
    if (connecting || !ip.trim()) return;
    connecting = true;
    root.querySelectorAll<HTMLButtonElement>('.pbtn, .recent button').forEach((b) => (b.disabled = true));
    message.classList.remove('error');
    message.textContent = `Connecting to ${ip.trim()}. If the camera asks, press OK on it.`;
    const ok = await actions.connect(ip);
    connecting = false;
    root.querySelectorAll<HTMLButtonElement>('.pbtn, .recent button').forEach((b) => (b.disabled = false));
    if (ok) message.textContent = '';
  }
  form.addEventListener('submit', (e) => {
    e.preventDefault();
    void connectTo(input.value);
  });

  const offs = [
    watch(store, (s) => s.error, (error) => {
      if (!error) return;
      message.textContent = error;
      message.classList.add('error');
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
          b.addEventListener('click', () => void connectTo(cam.ip));
          d.append(t, b);
          return d;
        }),
      );
    }),
    watch(store, (s) => s.settings?.recentCameras, (list = []) => {
      recent.hidden = list.length === 0;
      recentGroup.replaceChildren(
        ...list.map((rc) => {
          const r = rowButton(rc.model, rc.ip, 'chev');
          const b = r.querySelector('button')!;
          b.prepend(icon('cam', 'camglyph', '0 0 20 20'));
          b.addEventListener('click', () => void connectTo(rc.ip));
          return r;
        }),
      );
    }),
  ];

  let stopped = false;
  let timer = 0;
  const scan = async () => {
    const started = Date.now();
    try {
      const found = await discover(DISCOVER_MS);
      if (!stopped) store.set({ nearby: found });
    } catch {
      // Discovery is best effort; the address field always works.
    }
    if (!stopped) timer = window.setTimeout(scan, Math.max(0, REFRESH_MS - (Date.now() - started)));
  };
  void scan();

  return () => {
    stopped = true;
    window.clearTimeout(timer);
    offs.forEach((off) => off());
    parts.forEach((p) => p.remove());
  };
}
