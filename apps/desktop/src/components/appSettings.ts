// The App settings button and popover: Appearance and Layout as segmented
// controls, then Save to, then the shortcut hint. These settings are about
// Rimix itself; the camera's settings live in their own popover.

import type { Actions } from '../actions';
import { button, el, icon } from '../dom';
import type { AppSettings, Appearance, Layout } from '../ipc';
import { watch, type AppStore } from '../state';
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
): Segmented<T> {
  const r = el('div', 'r');
  r.setAttribute('role', 'listitem');
  const mini = el('div', 'mini');
  mini.setAttribute('role', 'radiogroup');
  mini.setAttribute('aria-label', label);
  const buttons = values.map((v) => {
    const b = button('', v);
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
    const current = store.get().settings;
    if (!current) {
      actions.toast('Settings did not load, so this change will not be kept');
      return;
    }
    const next = { ...current, ...patch };
    store.set({ settings: next });
    void actions.saveSettings(next);
  };
  const theme = segmented<Appearance>('Theme', 'Theme', ['Dark', 'Light', 'System'], (v) =>
    change({ appearance: v }),
  );
  const layout = segmented<Layout>('Controls', 'Controls layout', ['Dock', 'Ring'], (v) => change({ layout: v }));
  const saveTo = saveToSection(store, actions);
  const hint = el('div', 'hint');
  hint.innerHTML = KEYBOARD_GLYPH;
  hint.append(shortcutHint());
  pop.append(el('div', 'ttl', 'App settings'), sect('Appearance', theme.el), sect('Layout', layout.el), saveTo.el, hint);
  pop.hidden = arrow.hidden = true;

  const setOpen = (open: boolean) => {
    pop.hidden = arrow.hidden = !open;
    gear.setAttribute('aria-expanded', String(open));
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
  });

  return () => {
    off();
    saveTo.destroy();
    document.removeEventListener('keydown', onKey);
    [gear, arrow, pop].forEach((e) => e.remove());
  };
}
