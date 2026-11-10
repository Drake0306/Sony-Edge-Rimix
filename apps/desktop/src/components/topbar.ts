// The status capsule (Rimix, model, link, battery, firmware) and the App
// settings button with its popover. For now the popover holds Save to only.

import type { Actions } from '../actions';
import { button, el, icon } from '../dom';
import { watch, type AppStore } from '../state';
import { saveToSection } from './sidePanels';

function pair(...children: (Node | string)[]): HTMLElement {
  const p = el('span', 'pair');
  p.append(...children);
  return p;
}

export function mount(root: HTMLElement, store: AppStore, actions: Actions): () => void {
  const status = el('header', 'status glass capsule');
  status.setAttribute('aria-label', 'Camera status');
  const model = el('span', 'cam');
  const bars = el('span', 'bars');
  bars.setAttribute('aria-hidden', 'true');
  bars.append(el('i'), el('i'), el('i'), el('i'));
  const link = el('b');
  const batt = el('span', 'batt');
  batt.setAttribute('aria-hidden', 'true');
  const level = el('i');
  batt.append(level);
  const battText = el('b', 'num');
  const battPair = pair(batt, battText);
  const fw = el('b', 'num');
  status.append(el('span', 'brand', 'Rimix'), model, pair(bars, link), battPair, pair('Firmware ', fw));
  status.lastElementChild!.classList.add('fw');

  const gear = button('gear glass quiet', undefined, 'App settings');
  gear.setAttribute('aria-haspopup', 'dialog');
  gear.setAttribute('aria-expanded', 'false');
  gear.setAttribute('aria-controls', 'apppop');
  gear.append(icon('gearglyph', '', '0 0 20 20'), el('span', 'lbl', 'App settings'));

  const aparrow = el('div', 'aparrow');
  aparrow.setAttribute('aria-hidden', 'true');
  const app = el('div', 'apppop glass');
  app.id = 'apppop';
  app.setAttribute('role', 'dialog');
  app.setAttribute('aria-label', 'App settings');
  const saveTo = saveToSection(store, actions);
  app.append(el('div', 'ttl', 'App settings'), saveTo.el);
  app.hidden = aparrow.hidden = true;

  const setOpen = (open: boolean) => {
    app.hidden = aparrow.hidden = !open;
    gear.setAttribute('aria-expanded', String(open));
    if (open) app.querySelector<HTMLElement>('button')?.focus();
  };
  gear.addEventListener('click', () => setOpen(app.hidden));
  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'Escape' && !app.hidden) {
      setOpen(false);
      gear.focus();
    }
  };
  document.addEventListener('keydown', onKey);
  root.append(status, gear, aparrow, app);

  const off = watch(
    store,
    (s) => JSON.stringify([s.camera?.model, s.camera?.connected, s.camera?.battery, s.camera?.firmware, s.reconnecting]),
    (_, { camera, reconnecting }) => {
      model.textContent = camera?.model ?? '';
      link.textContent =
        reconnecting !== null ? 'Reconnecting' : camera?.connected === false ? 'Not connected' : 'Wi-Fi';
      bars.classList.toggle('lost', camera?.connected === false);
      const pct = camera?.battery ?? null;
      battPair.hidden = pct === null;
      battText.textContent = `${pct ?? 0}%`;
      level.style.width = `calc((100% - 3px) * ${Math.max(0, Math.min(100, pct ?? 0)) / 100})`;
      fw.textContent = camera?.firmware ?? '';
    },
  );

  return () => {
    off();
    saveTo.destroy();
    document.removeEventListener('keydown', onKey);
    [status, gear, aparrow, app].forEach((e) => e.remove());
  };
}
