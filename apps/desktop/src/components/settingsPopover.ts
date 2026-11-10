// The camera settings popover: the stacked settings list from the mockup,
// anchored above whichever dock chip opened it, or shown as a panel beside
// the control ring with a Done button.

import type { Actions } from '../actions';
import { button, el, icon, isTyping } from '../dom';
import type { CameraState, PropertySummary } from '../ipc';
import { findProp, isColourTemp, isUsable, PROP, sameRaw, SETTING_NAMES, shortText } from '../props';
import { watch, type AppStore } from '../state';

const PENDING_MS = 400;

interface Group {
  title: string;
  codes: (camera: CameraState | null) => number[];
  /** Chevrons for choices, minus/plus for values. */
  chevrons: boolean;
}

const GROUPS: Group[] = [
  {
    title: 'Exposure',
    codes: () => [PROP.mode, PROP.shutter, PROP.aperture, PROP.iso, PROP.exposureComp],
    chevrons: false,
  },
  {
    title: 'Image',
    codes: (c) =>
      isColourTemp(c) ? [PROP.whiteBalance, PROP.colourTemp, PROP.format] : [PROP.whiteBalance, PROP.format],
    chevrons: true,
  },
  { title: 'Focus and drive', codes: () => [PROP.focus, PROP.drive], chevrons: true },
];

/** A dock chip or a ring segment. */
export type Anchor = HTMLElement | SVGElement;

export interface PopoverOptions {
  /** A panel beside the ring: no arrow, a Done button, and the anchor marked `on`. */
  beside?: boolean;
}

export interface SettingsPopover {
  arrow: HTMLElement;
  pop: HTMLElement;
  /** Opens for the chip (or closes when it is already open there). */
  toggle(chip: Anchor, code: number | null, label: string): void;
  /** Opens from `anchor`, titled with `label`, highlighting the row for `highlightCode`. */
  open(anchor: Anchor, label: string, highlightCode?: number | null): void;
  close(): void;
  isOpen(): boolean;
  /** The code of the highlighted row; null when closed or opened without one. */
  activeCode(): number | null;
  /** Re-anchors to the open chip, e.g. after a resize. */
  place(): void;
  destroy(): void;
}

export function createSettingsPopover(
  store: AppStore,
  actions: Actions,
  { beside = false }: PopoverOptions = {},
): SettingsPopover {
  const arrow = el('div', 'arrow');
  arrow.setAttribute('aria-hidden', 'true');
  const pop = el(beside ? 'aside' : 'div', beside ? 'pop ringpanel glass' : 'pop glass');
  pop.id = 'pop';
  pop.setAttribute('role', 'dialog');
  pop.setAttribute('aria-label', 'Camera settings');
  const ttl = el('div', 'ttl', 'Settings ');
  const from = el('span');
  ttl.append(from);
  if (beside) {
    const done = button('done', 'Done');
    done.addEventListener('click', () => setOpen(null, null, ''));
    ttl.append(done);
  }
  // Dock chips are disclosure buttons; ring segments are toggle buttons.
  const [onClass, stateAttr] = beside ? ['on', 'aria-pressed'] : ['open', 'aria-expanded'];
  const body = el('div');
  pop.append(ttl, body);
  arrow.hidden = pop.hidden = true;

  let anchor: Anchor | null = null;
  let activeCode: number | null = null;
  let layoutKey = '';
  const rows = new Map<number, HTMLElement>();
  const pending = new Map<number, number>();

  function markPending(code: number): void {
    window.clearTimeout(pending.get(code));
    rows.get(code)?.classList.add('pending');
    pending.set(
      code,
      window.setTimeout(() => {
        pending.delete(code);
        rows.get(code)?.classList.remove('pending');
      }, PENDING_MS),
    );
  }

  function stepper(code: number, name: string, chevrons: boolean): HTMLElement {
    const step = el('div', 'step');
    const [down, up] = chevrons ? ['Previous', 'Next'] : ['Decrease', 'Increase'];
    for (const [delta, word, glyph] of [
      [-1, down, chevrons ? 'chl' : 'minus'],
      [1, up, chevrons ? 'chr' : 'plus'],
    ] as const) {
      const b = button('', undefined, `${word} ${name.toLowerCase()}`);
      b.append(icon(glyph, '', '0 0 10 10'));
      b.addEventListener('click', () => {
        actions.stepProperty(code, delta);
        markPending(code);
      });
      step.append(b);
    }
    return step;
  }

  function buildRow(code: number, prop: PropertySummary | undefined, chevrons: boolean): HTMLElement {
    const name = SETTING_NAMES[code];
    const row = el('div', 'r');
    row.setAttribute('role', 'listitem');
    row.dataset.code = String(code);
    row.append(el('span', 'k', name));
    if (code === PROP.mode && prop && prop.choices.length > 0 && prop.choices.length <= 4) {
      const mini = el('div', 'mini');
      mini.setAttribute('role', 'radiogroup');
      mini.setAttribute('aria-label', name);
      for (const choice of prop.choices) {
        const b = button('', choice.label);
        b.setAttribute('role', 'radio');
        b.addEventListener('click', () => {
          actions.setProperty(code, choice.raw);
          markPending(code);
        });
        mini.append(b);
      }
      row.append(mini);
    } else {
      row.append(el('span', 'v'), stepper(code, name, chevrons));
    }
    return row;
  }

  function updateRow(row: HTMLElement, prop: PropertySummary | undefined): void {
    const usable = isUsable(prop);
    row.classList.toggle('off', !usable);
    row.setAttribute('aria-disabled', String(!usable));
    row.querySelectorAll('button').forEach((b) => (b.disabled = !usable));
    const v = row.querySelector('.v');
    if (v) v.textContent = shortText(prop);
    row.querySelectorAll<HTMLButtonElement>('.mini button').forEach((b, i) => {
      const on = !!prop && !!prop.choices[i] && sameRaw(prop.choices[i].raw, prop.raw);
      b.classList.toggle('on', on);
      b.setAttribute('aria-checked', String(on));
    });
    const code = Number(row.dataset.code);
    row.classList.toggle('active', code === activeCode);
    if (code === activeCode) row.setAttribute('aria-current', 'true');
    else row.removeAttribute('aria-current');
    row.classList.toggle('pending', pending.has(code));
  }

  function render(camera: CameraState | null): void {
    const groups = GROUPS.map((g) => ({ ...g, list: g.codes(camera) }));
    const modeChoices = findProp(camera, PROP.mode)?.choices.length ?? 0;
    const key = JSON.stringify([groups.map((g) => g.list), modeChoices <= 4 && modeChoices > 0]);
    if (key !== layoutKey) {
      layoutKey = key;
      rows.clear();
      body.replaceChildren(
        ...groups.map((g) => {
          const sect = el('div', 'sect');
          const h = el('div', 'h');
          h.append(el('span', '', g.title));
          const group = el('div', 'group');
          group.setAttribute('role', 'list');
          for (const code of g.list) {
            const row = buildRow(code, findProp(camera, code), g.chevrons);
            rows.set(code, row);
            group.append(row);
          }
          sect.append(h, group);
          return sect;
        }),
      );
    }
    for (const [code, row] of rows) updateRow(row, findProp(camera, code));
  }

  const unwatch = watch(store, (s) => s.camera, (camera) => render(camera));

  function place(): void {
    const wrap = pop.offsetParent;
    if (beside || !anchor || pop.hidden || !wrap) return;
    const cr = anchor.getBoundingClientRect();
    const wr = wrap.getBoundingClientRect();
    const cx = cr.left - wr.left + cr.width / 2;
    pop.style.left = `${Math.max(0, cx - pop.offsetWidth / 2)}px`;
    arrow.style.left = `${cx}px`;
  }

  function setOpen(chip: Anchor | null, code: number | null, label: string): void {
    anchor?.classList.remove(onClass);
    anchor?.setAttribute(stateAttr, 'false');
    anchor = chip;
    activeCode = code;
    pop.hidden = !chip;
    arrow.hidden = beside || !chip;
    if (!chip) return;
    chip.classList.add(onClass);
    chip.setAttribute(stateAttr, 'true');
    from.textContent = `opened from ${label}`;
    render(store.get().camera);
    place();
  }

  return {
    arrow,
    pop,
    toggle: (chip, code, label) => setOpen(anchor === chip ? null : chip, code, label),
    open: (chip, label, highlightCode = null) => setOpen(chip, highlightCode, label),
    close: () => setOpen(null, null, ''),
    isOpen: () => !pop.hidden,
    activeCode: () => (pop.hidden ? null : activeCode),
    place,
    destroy: unwatch,
  };
}

const ARROW_STEP: Record<string, number> = { ArrowRight: 1, ArrowUp: 1, ArrowLeft: -1, ArrowDown: -1 };

/** Arrow keys step the setting whose row is highlighted; returns whether the key was used. */
export function stepOnArrow(
  e: KeyboardEvent,
  popover: SettingsPopover,
  store: AppStore,
  actions: Actions,
): boolean {
  const delta = ARROW_STEP[e.key];
  const code = popover.activeCode();
  if (delta === undefined || code === null || e.metaKey || e.ctrlKey || e.altKey || isTyping(e.target)) {
    return false;
  }
  if (!isUsable(findProp(store.get().camera, code))) return false;
  e.preventDefault();
  actions.stepProperty(code, delta);
  return true;
}
