// The status capsule: Rimix, model, link, battery and firmware. The App
// settings button beside it is in appSettings.ts.

import type { Actions } from '../actions';
import { reconnectingText } from '../copy';
import { el } from '../dom';
import { watch, type AppStore } from '../state';

function pair(...children: (Node | string)[]): HTMLElement {
  const p = el('span', 'pair');
  p.append(...children);
  return p;
}

export function mount(root: HTMLElement, store: AppStore, _actions: Actions): () => void {
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

  root.append(status);

  const off = watch(
    store,
    (s) => JSON.stringify([s.camera?.model, s.camera?.connected, s.camera?.battery, s.camera?.firmware, s.reconnecting]),
    (_, { camera, reconnecting }) => {
      model.textContent = camera?.model ?? '';
      link.textContent =
        reconnecting !== null ? reconnectingText(reconnecting) : camera?.connected === false ? 'Not connected' : 'Wi-Fi';
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
    status.remove();
  };
}
