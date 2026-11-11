// The camera settings popover: the stacked settings list from the mockup,
// anchored above whichever dock chip opened it, or shown as a panel beside
// the control ring with a Done button.

import type { Actions } from '../actions';
import { button, el, icon, isTyping } from '../dom';
import type { CameraState, PropertySummary } from '../ipc';
import { choicesIn, fLabel, F_STOPS, groupsOf, irisRadius, pickerFor, type Look } from '../pickers';
import {
  findProp,
  isColourTemp,
  isoStops,
  isUsable,
  kelvinRange,
  nearestIso,
  PROP,
  sameRaw,
  SETTING_NAMES,
  shortText,
} from '../props';
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
  /** The panel that unfolds under a row: a list of values, the f-stops, or the kelvin scale. */
  const panels = new Map<number, HTMLElement>();
  /** What each panel last drew, so it is rebuilt only when that changes. */
  const drawn = new Map<number, string>();
  /** The row unfolded now (one at a time), and the tab chosen by hand in each list. */
  let unfolded: number | null = null;
  const tabChosen = new Map<number, string>();
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

  /** The code the camera really keeps a setting under (Format has two). */
  const realCode = (code: number) => findProp(store.get().camera, code)?.code ?? code;

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
        actions.stepProperty(realCode(code), delta);
        markPending(code);
      });
      step.append(b);
    }
    return step;
  }

  /** Which kind of panel a row unfolds, if any. */
  function panelKind(code: number, prop: PropertySummary | undefined): 'list' | 'fstops' | 'kelvin' | null {
    if (!prop) return null;
    if (pickerFor(prop.code) && prop.choices.length > 0) return 'list';
    if (code === PROP.aperture && prop.choices.length === 0) return 'fstops';
    if (code === PROP.colourTemp) return 'kelvin';
    return null;
  }

  function buildRow(code: number, prop: PropertySummary | undefined, chevrons: boolean): HTMLElement {
    const name = SETTING_NAMES[code];
    const row = el('div', 'r');
    row.setAttribute('role', 'listitem');
    row.dataset.code = String(code);
    row.append(el('span', 'k', name));
    const kind = panelKind(code, prop);
    if (!kind) {
      row.append(el('span', 'v'), stepper(code, name, chevrons));
      return row;
    }
    // The value is a button: it shows the setting and unfolds its panel.
    const b = button('pickbtn');
    b.setAttribute('aria-expanded', 'false');
    b.setAttribute('aria-controls', `panel-${code}`);
    b.addEventListener('click', () => {
      unfolded = unfolded === code ? null : code;
      redrawPanels();
    });
    row.append(b);
    // Values with an order keep their plus and minus beside the button.
    if (kind !== 'list') row.append(stepper(code, name, chevrons));
    return row;
  }

  function buildPanel(code: number, kind: 'list' | 'fstops' | 'kelvin'): HTMLElement {
    const panel = el('div', `pick ${kind}`);
    panel.id = `panel-${code}`;
    panel.dataset.code = String(code);
    panel.hidden = true;
    if (kind === 'list') {
      const tabs = el('div', 'segc tabs');
      tabs.setAttribute('role', 'tablist');
      const list = el('div', 'picklist');
      list.setAttribute('role', 'radiogroup');
      panel.append(tabs, list);
    } else if (kind === 'fstops') {
      const grid = el('div', 'fgrid');
      grid.setAttribute('role', 'radiogroup');
      grid.setAttribute('aria-label', 'Aperture');
      panel.append(grid, el('p', 'note', 'The lens sets how wide and how narrow it goes; Rimix stops there.'));
    } else {
      buildKelvin(panel);
    }
    return panel;
  }

  /** A pictogram, a name and a badge: how a value looks in its row and in its list. */
  function lookParts(look: Look): HTMLElement[] {
    const parts = [icon(look.icon, 'mi', '0 0 20 20') as unknown as HTMLElement, el('span', 'mn', look.name)];
    if (look.badge) {
      const cap = el('span', 'cap', look.badge);
      if (look.badge.length > 1) cap.classList.add('wide');
      parts.push(cap);
    }
    return parts;
  }

  const chevron = () => icon('chr', 'chev', '0 0 10 10') as unknown as HTMLElement;

  /** An iris drawn as wide open as the f-number is. */
  function iris(hundredths: number): HTMLElement {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('viewBox', '0 0 20 20');
    svg.setAttribute('class', 'mi iris');
    svg.setAttribute('aria-hidden', 'true');
    const ring = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
    const hole = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
    for (const [c, r, cls] of [[ring, 8, 'ring'], [hole, irisRadius(hundredths), 'hole']] as const) {
      c.setAttribute('cx', '10');
      c.setAttribute('cy', '10');
      c.setAttribute('r', String(r));
      c.setAttribute('class', cls);
    }
    svg.append(ring, hole);
    return svg as unknown as HTMLElement;
  }

  function drawList(code: number, prop: PropertySummary, head: HTMLButtonElement, panel: HTMLElement, open: boolean): void {
    const spec = pickerFor(prop.code)!;
    const usable = isUsable(prop);
    const groups = groupsOf(spec, prop.choices);
    const here = spec.groupOf({ raw: prop.raw, label: prop.label, short: prop.short });
    const chosen = tabChosen.get(code);
    const ids = groups.map(([id]) => id);
    const tab = chosen && ids.includes(chosen) ? chosen : ids.includes(here) ? here : ids[0];
    const key = JSON.stringify([prop.raw, prop.label, prop.choices.map((c) => c.raw), tab, open, usable]);
    if (drawn.get(code) === key) return;
    drawn.set(code, key);

    const current = spec.look({ raw: prop.raw, label: prop.label, short: prop.short });
    head.replaceChildren(...lookParts(current), chevron());
    head.title = prop.label;
    head.setAttribute('aria-label', `${SETTING_NAMES[code]}, ${prop.label}. ${open ? 'Hide' : 'Show'} the list`);
    if (!open) return;

    const [tabs, list] = [panel.children[0] as HTMLElement, panel.children[1] as HTMLElement];
    tabs.setAttribute('aria-label', `Kind of ${spec.title.toLowerCase()}`);
    list.setAttribute('aria-label', spec.title);
    tabs.hidden = groups.length < 2;
    tabs.replaceChildren(
      ...groups.map(([id, title]) => {
        const b = button(id === tab ? 'on' : '', title);
        b.setAttribute('role', 'tab');
        b.setAttribute('aria-selected', String(id === tab));
        b.addEventListener('click', () => {
          tabChosen.set(code, id);
          redrawPanels();
        });
        return b;
      }),
    );
    list.replaceChildren(
      ...choicesIn(spec, prop.choices, tab).map((choice) => {
        const on = sameRaw(choice.raw, prop.raw);
        const b = button(on ? 'popt on' : 'popt');
        b.setAttribute('role', 'radio');
        b.setAttribute('aria-checked', String(on));
        b.disabled = !usable;
        b.append(...lookParts(spec.look(choice)));
        b.addEventListener('click', () => {
          actions.setProperty(prop.code, choice.raw);
          markPending(code);
        });
        return b;
      }),
    );
  }

  function drawFstops(code: number, prop: PropertySummary, head: HTMLButtonElement, panel: HTMLElement, open: boolean): void {
    const usable = isUsable(prop);
    const now = 'U16' in prop.raw ? prop.raw.U16 : 0;
    const key = JSON.stringify([now, open, usable]);
    if (drawn.get(code) === key) return;
    drawn.set(code, key);
    head.replaceChildren(iris(now || 400), el('span', 'mn', shortText(prop)), chevron());
    head.setAttribute('aria-label', `Aperture, ${shortText(prop)}. ${open ? 'Hide' : 'Show'} the f-stops`);
    if (!open) return;
    (panel.children[0] as HTMLElement).replaceChildren(
      ...F_STOPS.map((f) => {
        const b = button(f === now ? 'fstop on' : 'fstop');
        b.setAttribute('role', 'radio');
        b.setAttribute('aria-checked', String(f === now));
        b.disabled = !usable;
        b.append(iris(f), el('span', '', fLabel(f)));
        b.addEventListener('click', () => {
          actions.setProperty(prop.code, { U16: f });
          markPending(code);
        });
        return b;
      }),
    );
  }

  /** The kelvin scale: a strip from warm to cool to drag along, and a box to type a value into. */
  function buildKelvin(panel: HTMLElement): void {
    const scale = el('input', 'kscale');
    scale.type = 'range';
    scale.setAttribute('aria-label', 'Colour temperature');
    const ends = el('div', 'kends');
    ends.append(el('span', 'kmin'), el('span', 'know'), el('span', 'kmax'));
    const typed = el('div', 'r custom ktype');
    const input = el('input');
    input.type = 'number';
    input.inputMode = 'numeric';
    input.setAttribute('aria-label', 'Colour temperature in kelvin');
    const range = el('span', 'range');
    typed.append(el('span', 'k dim', 'Kelvin'), input, range);
    panel.append(scale, ends, typed);

    const set = (value: number) => {
      const prop = findProp(store.get().camera, PROP.colourTemp);
      if (!prop || !isUsable(prop) || !Number.isFinite(value)) return;
      const { min, max, step } = kelvinRange(prop);
      // Onto the camera's own steps, inside its range.
      const kelvin = Math.min(max, Math.max(min, min + Math.round((value - min) / step) * step));
      input.value = String(kelvin);
      scale.value = String(kelvin);
      range.textContent = kelvin === value ? `${min} to ${max}` : `Nearest the camera has: ${kelvin}`;
      actions.setProperty(prop.code, { U16: kelvin });
      markPending(PROP.colourTemp);
    };
    // While dragging only the number follows; the camera is told on release.
    scale.addEventListener('input', () => {
      (ends.children[1] as HTMLElement).textContent = `${scale.value}K`;
      input.value = scale.value;
    });
    scale.addEventListener('change', () => set(Number(scale.value)));
    input.addEventListener('keydown', (e) => {
      e.stopPropagation();
      if (e.key === 'Enter') set(Number(input.value));
    });
    input.addEventListener('change', () => set(Number(input.value)));
  }

  function drawKelvin(code: number, prop: PropertySummary, head: HTMLButtonElement, panel: HTMLElement, open: boolean): void {
    const usable = isUsable(prop);
    const now = 'U16' in prop.raw ? prop.raw.U16 : 0;
    const { min, max, step } = kelvinRange(prop);
    const key = JSON.stringify([now, open, usable, min, max, step]);
    if (drawn.get(code) === key) return;
    drawn.set(code, key);
    const swatch = el('span', 'kswatch');
    swatch.style.setProperty('--at', `${max > min ? ((now - min) / (max - min)) * 100 : 50}%`);
    head.replaceChildren(swatch, el('span', 'mn', shortText(prop)), chevron());
    head.setAttribute('aria-label', `Colour temperature, ${shortText(prop)}. ${open ? 'Hide' : 'Show'} the scale`);
    if (!open) return;
    const scale = panel.querySelector<HTMLInputElement>('.kscale')!;
    const input = panel.querySelector<HTMLInputElement>('.ktype input')!;
    for (const field of [scale, input]) {
      field.min = String(min);
      field.max = String(max);
      field.step = String(step);
      field.value = String(now);
      field.disabled = !usable;
    }
    panel.querySelector('.kmin')!.textContent = `${min}K`;
    panel.querySelector('.know')!.textContent = `${now}K`;
    panel.querySelector('.kmax')!.textContent = `${max}K`;
    panel.querySelector('.ktype .range')!.textContent = `${min} to ${max}`;
  }

  /** Draws every row that has a panel: its button always, its panel while it is the one unfolded. */
  function redrawPanels(): void {
    const camera = store.get().camera;
    for (const [code, panel] of panels) {
      const prop = findProp(camera, code);
      const row = rows.get(code);
      const head = row?.querySelector<HTMLButtonElement>('.pickbtn');
      if (!prop || !row || !head) continue;
      const open = unfolded === code;
      head.setAttribute('aria-expanded', String(open));
      row.classList.toggle('unfolded', open);
      panel.hidden = !open;
      if (panel.classList.contains('list')) drawList(code, prop, head, panel, open);
      else if (panel.classList.contains('fstops')) drawFstops(code, prop, head, panel, open);
      else drawKelvin(code, prop, head, panel, open);
    }
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
    // The button that unfolds a panel stays live, to look at what there is.
    row.querySelectorAll<HTMLButtonElement>('button:not(.pickbtn)').forEach((b) => (b.disabled = !usable));
    const v = row.querySelector('.v');
    if (v) v.textContent = shortText(prop);
    if (row === isoCustom) {
      const custom = row.querySelector<HTMLInputElement>('input')!;
      const stops = isoStops(prop);
      const [min, max] = [stops[0], stops[stops.length - 1]];
      custom.disabled = !usable || stops.length === 0;
      custom.min = stops.length ? String(min) : '';
      custom.max = stops.length ? String(max) : '';
      const range = row.querySelector('.range')!;
      range.textContent = row.dataset.note || (stops.length ? `${min} to ${max}` : '');
    }
    const code = Number(row.dataset.code);
    row.classList.toggle('active', code === activeCode);
    if (code === activeCode) row.setAttribute('aria-current', 'true');
    else row.removeAttribute('aria-current');
    row.classList.toggle('pending', pending.has(code));
  }

  function render(camera: CameraState | null): void {
    const groups = GROUPS.map((g) => ({ ...g, list: g.codes(camera) }));
    // Which rows there are, and which of them unfold what: a row's kind can
    // change when the camera starts listing values for it.
    const kinds = groups.map((g) => g.list.map((code) => [code, panelKind(code, findProp(camera, code))]));
    const key = JSON.stringify(kinds);
    if (key !== layoutKey) {
      layoutKey = key;
      rows.clear();
      panels.clear();
      drawn.clear();
      isoCustom = null;
      body.replaceChildren(
        ...groups.map((g) => {
          const sect = el('div', 'sect');
          const h = el('div', 'h');
          h.append(el('span', '', g.title));
          const group = el('div', 'group');
          group.setAttribute('role', 'list');
          for (const code of g.list) {
            const prop = findProp(camera, code);
            const row = buildRow(code, prop, g.chevrons);
            rows.set(code, row);
            group.append(row);
            const kind = panelKind(code, prop);
            if (kind) {
              const panel = buildPanel(code, kind);
              panels.set(code, panel);
              group.append(panel);
            }
            if (code === PROP.iso) {
              isoCustom = customIsoRow();
              group.append(isoCustom);
            }
          }
          sect.append(h, group);
          return sect;
        }),
      );
    }
    for (const [code, row] of rows) updateRow(row, findProp(camera, code));
    if (isoCustom) updateRow(isoCustom, findProp(camera, PROP.iso));
    redrawPanels();
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
    // Opened from a chip whose setting has a panel, that panel is unfolded.
    unfolded = code;
    tabChosen.clear();
    drawn.clear();
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
  actions.stepProperty(findProp(store.get().camera, code)?.code ?? code, delta);
  return true;
}
