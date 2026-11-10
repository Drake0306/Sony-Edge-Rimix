// The control ring (layout: ring), ported from design/mockups/20-rimix-final.html:
// ten value segments round the edge, Record and Focus as inner arcs, Shoot in
// the hub. A segment opens the camera settings list as a panel beside the
// ring. Presets and the live view status sit in a bar at the bottom left.

import type { Actions } from '../actions';
import { el } from '../dom';
import { chipValue, CHIP_CODES, findProp, PROP, SETTING_NAMES } from '../props';
import { watch, type AppStore } from '../state';
import { statusText } from './dock';
import { presetsControl } from './presetsControl';
import { createSettingsPopover } from './settingsPopover';

/** Segment outline and value position in the 400 by 400 viewBox, in CHIP_CODES order then Picture profile. */
const SEGMENTS: [d: string, x: number, y: number][] = [
  ['M84.99 353.74 A192 192 0 0 1 18.24 261.88 L93.98 236.09 A112 112 0 0 0 132.91 289.68 Z', 77.03, 287.34],
  ['M16.59 256.78 A192 192 0 0 1 16.59 143.22 L93.01 166.88 A112 112 0 0 0 93.01 233.12 Z', 48, 198],
  ['M18.24 138.12 A192 192 0 0 1 84.99 46.26 L132.91 110.32 A112 112 0 0 0 93.98 163.91 Z', 77.03, 108.66],
  ['M89.32 43.11 A192 192 0 0 1 197.32 8.02 L198.44 88.01 A112 112 0 0 0 135.44 108.48 Z', 153.03, 53.44],
  ['M202.68 8.02 A192 192 0 0 1 310.68 43.11 L264.56 108.48 A112 112 0 0 0 201.56 88.01 Z', 246.97, 53.44],
  ['M315.01 46.26 A192 192 0 0 1 381.76 138.12 L306.02 163.91 A112 112 0 0 0 267.09 110.32 Z', 322.97, 108.66],
  ['M383.41 143.22 A192 192 0 0 1 383.41 256.78 L306.99 233.12 A112 112 0 0 0 306.99 166.88 Z', 352, 198],
  ['M381.76 261.88 A192 192 0 0 1 315.01 353.74 L267.09 289.68 A112 112 0 0 0 306.02 236.09 Z', 322.97, 287.34],
  ['M310.68 356.89 A192 192 0 0 1 202.68 391.98 L201.56 311.99 A112 112 0 0 0 264.56 291.52 Z', 246.97, 342.56],
  ['M197.32 391.98 A192 192 0 0 1 89.32 356.89 L135.44 291.52 A112 112 0 0 0 198.44 311.99 Z', 153.03, 342.56],
];

const TICKS =
  '<g class="ticks"><line x1="86.26" y1="356.54" x2="90.67" y2="350.48"/><line x1="15.97" y1="259.79" x2="23.10" y2="257.48"/><line x1="15.97" y1="140.21" x2="23.10" y2="142.52"/><line x1="86.26" y1="43.46" x2="90.67" y2="49.52"/><line x1="200.00" y1="6.50" x2="200.00" y2="14.00"/><line x1="313.74" y1="43.46" x2="309.33" y2="49.52"/><line x1="384.03" y1="140.21" x2="376.90" y2="142.52"/><line x1="384.03" y1="259.79" x2="376.90" y2="257.48"/><line x1="313.74" y1="356.54" x2="309.33" y2="350.48"/><line x1="200.00" y1="393.50" x2="200.00" y2="386.00"/></g>';

const SEGMENT_SVG = SEGMENTS.map(
  ([d, x, y]) =>
    `<g class="seg" role="button" tabindex="0" aria-pressed="false"><path d="${d}"/><text class="v" x="${x}" y="${y}"></text><text class="k" x="${x}" y="${y + 15}"></text></g>`,
).join('');

const HUB_SVG =
  '<g class="act rec" role="button" tabindex="0" aria-label="Record"><path d="M97.58 181.94 A104 104 0 0 1 302.42 181.94 L261.06 189.23 A62 62 0 0 0 138.94 189.23 Z"/><circle class="dot" cx="176.00" cy="117.00" r="5"/><text x="206.00" y="121.50">Record</text></g>' +
  '<g class="act af" role="button" tabindex="0" aria-label="Focus"><path d="M302.42 218.06 A104 104 0 0 1 97.58 218.06 L138.94 210.77 A62 62 0 0 0 261.06 210.77 Z"/><g class="afg" transform="translate(174.00 276.00)"><rect x="0" y="0" width="14" height="14" rx="3"/><circle cx="7" cy="7" r="2.2"/></g><text x="206.00" y="287.50">Focus</text></g>' +
  '<g class="shoot" role="button" tabindex="0" aria-label="Shoot"><circle class="halo" cx="200" cy="200" r="52"/><circle class="disc" cx="200" cy="200" r="44"/><text x="200" y="205">Shoot</text></g>';

/** Clicks, Enter and Space all activate; keys stop here so Space on a ring control never also shoots. */
function activate(target: Element, run: () => void): void {
  target.addEventListener('click', run);
  target.addEventListener('keydown', (e) => {
    const k = e as KeyboardEvent;
    if (k.key !== 'Enter' && k.key !== ' ') return;
    k.preventDefault();
    k.stopPropagation();
    if (!k.repeat) run();
  });
}

const segmentValue = (camera: Parameters<typeof chipValue>[0], code: number) => {
  const v = chipValue(camera, code);
  return code === PROP.iso && v !== '—' ? `ISO ${v}` : v;
};

export function mount(root: HTMLElement, store: AppStore, actions: Actions): () => void {
  const bar = el('div', 'ringbar glass primary capsule');
  bar.setAttribute('role', 'group');
  bar.setAttribute('aria-label', 'Presets and live view status');
  const live = el('span', 'live');
  const presets = presetsControl(store, actions);
  bar.append(presets.el, live);

  const popover = createSettingsPopover(store, actions, { beside: true });
  const ring = el('div', 'ring');
  ring.setAttribute('role', 'group');
  ring.setAttribute('aria-label', 'Control ring');
  ring.innerHTML = `<svg viewBox="0 0 400 400" xmlns="http://www.w3.org/2000/svg">${TICKS}${SEGMENT_SVG}${HUB_SVG}</svg>`;
  root.append(bar, popover.pop, ring);

  const groups = [...ring.querySelectorAll<SVGGElement>('.seg')];
  const bound = CHIP_CODES.map((code, i) => {
    const g = groups[i];
    const label = SETTING_NAMES[code];
    g.dataset.code = String(code);
    g.setAttribute('aria-controls', 'pop');
    g.querySelector('.k')!.textContent = label;
    activate(g, () => (g.classList.contains('on') ? popover.close() : popover.open(g, label, code)));
    return { code, g };
  });
  const profile = groups[9];
  profile.classList.add('off', 'disabled');
  profile.setAttribute('aria-disabled', 'true');
  profile.removeAttribute('aria-pressed');
  profile.removeAttribute('tabindex');
  profile.setAttribute('aria-label', 'Picture profile, not available yet');
  profile.querySelector('.k')!.textContent = 'Picture profile';
  profile.querySelector('.v')!.textContent = '—';

  const record = ring.querySelector('.act.rec')!;
  activate(record, () => actions.toggleRecord());
  activate(ring.querySelector('.act.af')!, () => actions.focus());
  activate(ring.querySelector('.shoot')!, () => actions.shoot());

  const offs = [
    watch(store, (s) => s.camera, (camera) => {
      for (const { code, g } of bound) {
        const prop = findProp(camera, code);
        const value = segmentValue(camera, code);
        g.querySelector('.v')!.textContent = value;
        g.classList.toggle('off', !prop || prop.enabled !== 'On');
        g.setAttribute('aria-label', `${SETTING_NAMES[code]}, ${value}`);
      }
      record.classList.toggle('recording', !!camera?.recording);
      record.setAttribute('aria-pressed', String(!!camera?.recording));
    }),
    watch(store, (s) => s.status, (status) => (live.textContent = statusText(status))),
  ];
  const tick = window.setInterval(() => (live.textContent = statusText(store.get().status)), 500);
  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'Escape' && popover.isOpen()) {
      const seg = ring.querySelector<SVGGElement>('.seg.on');
      popover.close();
      seg?.focus();
    }
  };
  document.addEventListener('keydown', onKey);

  return () => {
    offs.forEach((off) => off());
    popover.destroy();
    presets.destroy();
    window.clearInterval(tick);
    document.removeEventListener('keydown', onKey);
    [bar, popover.pop, ring].forEach((e) => e.remove());
  };
}
