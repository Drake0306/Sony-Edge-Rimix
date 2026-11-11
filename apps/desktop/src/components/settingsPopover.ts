// The camera settings popover: the stacked settings list from the mockup,
// anchored above whichever dock chip opened it, or shown as a panel beside
// the control ring with a Done button.

import type { Actions } from '../actions';
import { button, el, icon, isTyping } from '../dom';
import type { CameraState, PropertySummary } from '../ipc';
import { findProp, isColourTemp, isoStops, isUsable, nearestIso, PROP, sameRaw, SETTING_NAMES, shortText } from '../props';
import { FAMILIES, familiesOf, modeInfo, modeName, modesIn, type ModeFamily } from '../modes';
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
  /** The typing row under ISO; it shares ISO's property but is not its row. */
  let isoCustom: HTMLElement | null = null;
  /** The mode picker under the Mode row, whether it is unfolded, and the tab chosen by hand. */
  let modePick: HTMLElement | null = null;
  let modeOpen = false;
  let modeTab: ModeFamily | null = null;
  /** What the picker last drew, so it is rebuilt only when that changes. */
  let modeKey = '';
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
    if (code === PROP.mode && prop && prop.choices.length > 0) {
      // Mode is chosen from a list, not stepped through: see `drawMode`.
      const b = button('modebtn');
      b.setAttribute('aria-expanded', 'false');
      b.setAttribute('aria-controls', 'modepick');
      b.append(icon('m-photo', 'mi', '0 0 20 20'), el('span', 'mn'), el('span', 'cap'), icon('chr', 'chev', '0 0 10 10'));
      b.addEventListener('click', () => {
        modeOpen = !modeOpen;
        modeTab = null;
        drawMode(findProp(store.get().camera, PROP.mode));
      });
      row.append(b);
    } else {
      row.append(el('span', 'v'), stepper(code, name, chevrons));
    }
    return row;
  }

  /** The list of modes under the Mode row: tabs for the kinds of shooting, then the modes of the tab showing. */
  function buildModePick(): HTMLElement {
    const pick = el('div', 'modepick');
    pick.id = 'modepick';
    const tabs = el('div', 'segc tabs');
    tabs.setAttribute('role', 'tablist');
    tabs.setAttribute('aria-label', 'Kind of shooting');
    const list = el('div', 'modelist');
    list.setAttribute('role', 'radiogroup');
    list.setAttribute('aria-label', 'Shooting mode');
    pick.append(tabs, list);
    pick.hidden = true;
    return pick;
  }

  /** A pictogram, a name and a dial cap: what a mode looks like in the row and in the list. */
  function modeParts(raw: PropertySummary['raw'], name: string): HTMLElement[] {
    const info = modeInfo(raw);
    const cap = el('span', 'cap', info.badge);
    if (info.badge.length > 1) cap.classList.add('wide');
    return [icon(info.icon, 'mi', '0 0 20 20') as unknown as HTMLElement, el('span', 'mn', name), cap];
  }

  function drawMode(prop: PropertySummary | undefined): void {
    const row = rows.get(PROP.mode);
    const head = row?.querySelector<HTMLButtonElement>('.modebtn');
    if (!row || !head || !modePick || !prop) return;
    const usable = isUsable(prop);
    const here = modeInfo(prop.raw);
    const families = familiesOf(prop.choices);
    const tab = modeTab && families.includes(modeTab) ? modeTab : families.includes(here.family) ? here.family : families[0];
    const key = JSON.stringify([prop.raw, prop.label, prop.choices.map((c) => c.raw), tab, modeOpen, usable]);
    if (key === modeKey) return;
    modeKey = key;

    const name = prop.label.replace(/ \((Movie|S&Q|High frame rate)\)$/, '');
    head.replaceChildren(...modeParts(prop.raw, name), icon('chr', 'chev', '0 0 10 10'));
    head.title = prop.label;
    head.setAttribute('aria-expanded', String(modeOpen));
    head.setAttribute('aria-label', `Mode, ${prop.label}. ${modeOpen ? 'Hide' : 'Show'} the list of modes`);
    row.classList.toggle('unfolded', modeOpen);
    modePick.hidden = !modeOpen;
    if (!modeOpen) return;

    const [tabs, list] = [modePick.children[0] as HTMLElement, modePick.children[1] as HTMLElement];
    tabs.hidden = families.length < 2;
    tabs.replaceChildren(
      ...FAMILIES.filter(([f]) => families.includes(f)).map(([family, title]) => {
        const b = button(family === tab ? 'on' : '', title);
        b.setAttribute('role', 'tab');
        b.setAttribute('aria-selected', String(family === tab));
        b.addEventListener('click', () => {
          modeTab = family;
          drawMode(findProp(store.get().camera, PROP.mode));
        });
        return b;
      }),
    );
    list.replaceChildren(
      ...modesIn(prop.choices, tab).map((choice) => {
        const on = sameRaw(choice.raw, prop.raw);
        const b = button(on ? 'mopt on' : 'mopt');
        b.setAttribute('role', 'radio');
        b.setAttribute('aria-checked', String(on));
        b.disabled = !usable;
        b.append(...modeParts(choice.raw, modeName(choice)));
        b.addEventListener('click', () => {
          actions.setProperty(PROP.mode, choice.raw);
          markPending(PROP.mode);
        });
        return b;
      }),
    );
  }

  /**
   * The row under ISO for typing a value. A camera takes only its own
   * stops, so the range is shown and what is typed goes to the nearest one.
   */
  function customIsoRow(): HTMLElement {
    const row = el('div', 'r custom');
    row.setAttribute('role', 'listitem');
    row.dataset.code = String(PROP.iso);
    const input = el('input');
    input.type = 'number';
    input.inputMode = 'numeric';
    input.placeholder = 'Type a value';
    input.setAttribute('aria-label', 'Custom ISO');
    const range = el('span', 'range');
    const apply = () => {
      const prop = findProp(store.get().camera, PROP.iso);
      const nearest = nearestIso(prop, Number(input.value));
      if (!nearest || !isUsable(prop)) return;
      const typed = Number(input.value);
      actions.setProperty(PROP.iso, nearest.raw);
      markPending(PROP.iso);
      input.value = String(nearest.value);
      row.dataset.note = nearest.value === typed ? '' : `Nearest the camera has: ${nearest.value}`;
      updateRow(row, prop);
    };
    input.addEventListener('keydown', (e) => {
      // The arrow keys belong to the box while it is typed in.
      e.stopPropagation();
      if (e.key === 'Enter') apply();
    });
    input.addEventListener('change', apply);
    row.append(el('span', 'k dim', 'Custom'), input, range);
    return row;
  }

  function updateRow(row: HTMLElement, prop: PropertySummary | undefined): void {
    const usable = isUsable(prop);
    row.classList.toggle('off', !usable);
    row.setAttribute('aria-disabled', String(!usable));
    row.querySelectorAll<HTMLButtonElement>('button:not(.modebtn)').forEach((b) => (b.disabled = !usable));
    const v = row.querySelector('.v');
    if (v) v.textContent = shortText(prop);
    const custom = row.querySelector<HTMLInputElement>('input');
    if (custom) {
      const stops = isoStops(prop);
      const [min, max] = [stops[0], stops[stops.length - 1]];
      custom.disabled = !usable || stops.length === 0;
      custom.min = stops.length ? String(min) : '';
      custom.max = stops.length ? String(max) : '';
      const range = row.querySelector('.range')!;
      range.textContent = row.dataset.note || (stops.length ? `${min} to ${max}` : '');
    }
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
    const key = JSON.stringify([groups.map((g) => g.list), (findProp(camera, PROP.mode)?.choices.length ?? 0) > 0]);
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
            if (code === PROP.iso) {
              isoCustom = customIsoRow();
              group.append(isoCustom);
            }
            if (code === PROP.mode) {
              modePick = buildModePick();
              modeKey = '';
              group.append(modePick);
            }
          }
          sect.append(h, group);
          return sect;
        }),
      );
    }
    for (const [code, row] of rows) updateRow(row, findProp(camera, code));
    if (isoCustom) updateRow(isoCustom, findProp(camera, PROP.iso));
    drawMode(findProp(camera, PROP.mode));
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
    modeOpen = code === PROP.mode;
    modeTab = null;
    modeKey = '';
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
  // Another control (a menu, say) has the key already.
  if (e.defaultPrevented) return false;
  if (e.target instanceof Element && e.target.closest('[role="menu"]')) return false;
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
