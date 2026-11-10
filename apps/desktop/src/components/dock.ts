// The floating glass dock: ten setting chips, presets, Record, Shoot, Focus
// and the live view status. Chips open the camera settings popover.

import type { Actions } from '../actions';
import { button, el } from '../dom';
import { chipValue, CHIP_CODES, findProp, SETTING_NAMES } from '../props';
import { watch, type AppState, type AppStore } from '../state';
import { createSettingsPopover } from './settingsPopover';

const PRESETS_SOON = 'Presets arrive in the next version';
const FOCUS_GLYPH =
  '<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><path d="M3 7V5a2 2 0 0 1 2-2h2M13 3h2a2 2 0 0 1 2 2v2M17 13v2a2 2 0 0 1-2 2h-2M7 17H5a2 2 0 0 1-2-2v-2"/><circle cx="10" cy="10" r="2.2"/></svg>';

/** "Live view 24 fps, last shot 0.8 s ago"; the second part only after a shot. */
export function statusText(status: AppState['status'], now = Date.now()): string {
  const fps = `Live view ${Math.round(status.fps)} fps`;
  if (status.lastShotAt === null) return fps;
  const secs = Math.max(0, (now - status.lastShotAt) / 1000);
  return `${fps}, last shot ${secs < 10 ? secs.toFixed(1) : Math.round(secs)} s ago`;
}

function chip(label: string): HTMLButtonElement {
  const b = button('chip');
  b.append(el('small', '', label), el('b'));
  return b;
}

/** The four preset slots; presets arrive in the next version. */
export function presets(): HTMLElement {
  const segc = el('div', 'segc');
  segc.setAttribute('role', 'radiogroup');
  segc.setAttribute('aria-label', 'Presets');
  segc.title = PRESETS_SOON;
  ['C1', 'C2', 'C3', 'C4'].forEach((name, i) => {
    const b = button(i === 0 ? 'on' : '');
    b.setAttribute('role', 'radio');
    b.setAttribute('aria-checked', String(i === 0));
    b.setAttribute('aria-disabled', 'true');
    b.title = PRESETS_SOON;
    b.append(el('small', '', name));
    if (i === 0) b.append('Talking head');
    else b.setAttribute('aria-label', `Preset ${name}, empty`);
    segc.append(b);
  });
  return segc;
}

function action(label: string, extra: string, glyph: (c: HTMLElement) => void, run: () => void) {
  const b = button(`act ${extra}`.trim());
  const c = el('span', 'c');
  glyph(c);
  b.append(c, label);
  b.addEventListener('click', run);
  return b;
}

export function mount(root: HTMLElement, store: AppStore, actions: Actions): () => void {
  const wrap = el('div', 'dockwrap');
  const popover = createSettingsPopover(store, actions);
  const dock = el('div', 'dock glass primary');
  dock.setAttribute('role', 'toolbar');
  dock.setAttribute('aria-label', 'Camera controls');

  const chips = el('div', 'chips');
  chips.setAttribute('role', 'group');
  chips.setAttribute('aria-label', 'Exposure values');
  const bound = CHIP_CODES.map((code) => {
    const label = SETTING_NAMES[code];
    const b = chip(label);
    b.setAttribute('aria-expanded', 'false');
    b.setAttribute('aria-controls', 'pop');
    b.addEventListener('click', () => popover.toggle(b, code, label));
    chips.append(b);
    return { code, b };
  });
  const profile = chip('Picture profile');
  profile.querySelector('b')!.textContent = '—';
  profile.disabled = true;
  profile.classList.add('off');
  profile.title = 'Not available yet';
  chips.append(profile);

  const record = action('Record', '', (c) => c.append(el('span', 'rec')), () => actions.toggleRecord());
  const shoot = action('Shoot', 'shoot', () => {}, () => actions.shoot());
  const focus = action('Focus', '', (c) => (c.innerHTML = FOCUS_GLYPH), () => actions.focus());
  const acts = el('div', 'actions');
  acts.append(record, shoot, focus);

  const left = el('div', 'left');
  left.append(presets());
  const right = el('div', 'right');
  right.setAttribute('aria-live', 'off');
  const row = el('div', 'dockrow');
  row.append(left, acts, right);
  dock.append(chips, row);
  wrap.append(popover.arrow, popover.pop, dock);
  root.append(wrap);

  const offs = [
    watch(store, (s) => s.camera, (camera) => {
      for (const { code, b } of bound) {
        const prop = findProp(camera, code);
        b.querySelector('b')!.textContent = chipValue(camera, code);
        b.classList.toggle('off', !prop || prop.enabled !== 'On');
        b.setAttribute('aria-label', `${SETTING_NAMES[code]}, ${chipValue(camera, code)}`);
      }
      record.classList.toggle('recording', !!camera?.recording);
      record.setAttribute('aria-pressed', String(!!camera?.recording));
    }),
    watch(store, (s) => s.status, (status) => (right.textContent = statusText(status))),
  ];
  const tick = window.setInterval(() => (right.textContent = statusText(store.get().status)), 500);
  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'Escape' && popover.isOpen()) {
      const chipEl = wrap.querySelector<HTMLElement>('.chip.open');
      popover.close();
      chipEl?.focus();
    }
  };
  const onResize = () => popover.place();
  document.addEventListener('keydown', onKey);
  window.addEventListener('resize', onResize);
  void document.fonts?.ready.then(onResize);

  return () => {
    offs.forEach((off) => off());
    popover.destroy();
    window.clearInterval(tick);
    document.removeEventListener('keydown', onKey);
    window.removeEventListener('resize', onResize);
    wrap.remove();
  };
}
