// The status capsule: Rimix, model (and how many cameras, when several), link (with a lock through Access
// Authentication), battery and firmware. The App
// settings button beside it is in appSettings.ts.

import type { Actions } from '../actions';
import { reconnectingText } from '../copy';
import { el } from '../dom';
import { cameraCount, watch, type AppStore } from '../state';

const LOCK_GLYPH =
  '<svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" aria-hidden="true"><rect x="2.5" y="5.5" width="7" height="5" rx="1.2"/><path d="M4 5.5V4a2 2 0 0 1 4 0v1.5"/></svg>';

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
  const lock = el('span', 'lock');
  lock.setAttribute('role', 'img');
  lock.setAttribute('aria-label', 'Connected through Access Authentication');
  lock.innerHTML = LOCK_GLYPH;
  lock.hidden = true;
  const batt = el('span', 'batt');
  batt.setAttribute('aria-hidden', 'true');
  const level = el('i');
  batt.append(level);
  const battText = el('b', 'num');
  const battPair = pair(batt, battText);
  const fw = el('b', 'num');
  const fwPair = pair('Firmware ', fw);
  fwPair.classList.add('fw');
  status.append(el('span', 'brand', 'Rimix'), model, pair(bars, link, lock), battPair, fwPair);

  root.append(status);

  const off = watch(
    store,
    (s) =>
      JSON.stringify([
        s.camera?.model,
        s.camera?.connected,
        s.camera?.secured,
        s.camera?.battery,
        s.camera?.firmware,
        s.camera?.generation,
        s.reconnecting,
        cameraCount(s),
      ]),
    (_, s) => {
      const { camera, reconnecting } = s;
      const count = cameraCount(s);
      model.textContent = (camera?.model ?? '') + (count > 1 ? ` · ${count} cameras` : '');
      link.textContent =
        reconnecting !== null ? reconnectingText(reconnecting) : camera?.connected === false ? 'Not connected' : 'Wi-Fi';
      bars.classList.toggle('lost', camera?.connected === false);
      lock.hidden = !(camera?.connected && camera.secured);
      const pct = camera?.battery ?? null;
      battPair.hidden = pct === null;
      battText.textContent = `${pct ?? 0}%`;
      level.style.width = `calc((100% - 3px) * ${Math.max(0, Math.min(100, pct ?? 0)) / 100})`;
      fw.textContent = camera?.firmware ?? '';
      fwPair.title = camera ? `Firmware ${camera.firmware}, generation ${camera.generation}` : '';
    },
  );

  return () => {
    off();
    status.remove();
  };
}
