// The Scopes and timer panel's Interval timer row and its glass popover:
// a period in seconds, an optional number of shots, and Start or Stop.

import type { Actions } from '../actions';
import { button, el } from '../dom';
import { MIN_EVERY_S, type IntervalPlan, type IntervalRun } from '../interval';
import { watch, type AppStore } from '../state';
import { rowButton } from './sidePanels';

const DEFAULT_EVERY_S = 5;

/** The row's value: "Off", "Every 5 s" or "Every 5 s, 20 shots". */
export function rowValue(run: IntervalRun | null): string {
  if (!run?.running) return 'Off';
  const { everyS, count } = run.plan;
  const shots = count === null ? '' : `, ${count} ${count === 1 ? 'shot' : 'shots'}`;
  return `Every ${everyS} s${shots}`;
}

let ids = 0;

/** Appends the row to `root` (a Scopes list); the popover floats beside the left column. */
export function mount(root: HTMLElement, store: AppStore, actions: Actions): () => void {
  const row = rowButton('Interval timer', 'Off', 'chev');
  const open = row.querySelector('button')!;
  const value = open.querySelector<HTMLElement>('.v')!;
  const popId = `interval-pop-${++ids}`;
  open.setAttribute('aria-haspopup', 'dialog');
  open.setAttribute('aria-expanded', 'false');
  open.setAttribute('aria-controls', popId);
  root.append(row);

  const pop = el('form', 'interval-pop glass primary');
  pop.id = popId;
  pop.setAttribute('role', 'dialog');
  pop.setAttribute('aria-label', 'Interval timer');
  pop.hidden = true;
  // Out-of-range values are clamped on Start rather than refused by the browser.
  pop.noValidate = true;
  const field = (label: string, unit: string) => {
    const wrap = el('label', 'field');
    const input = el('input');
    input.type = 'number';
    input.step = '1';
    input.setAttribute('aria-label', label);
    wrap.append(el('span', 'k', label), input, el('span', 'unit', unit));
    return { wrap, input };
  };
  const every = field('Every', 's');
  every.input.min = String(MIN_EVERY_S);
  every.input.value = String(DEFAULT_EVERY_S);
  const shots = field('Shots', '');
  shots.input.min = '1';
  shots.input.placeholder = 'Unlimited';
  const startBtn = button('pbtn', 'Start');
  startBtn.type = 'submit';
  const stopBtn = button('pbtn', 'Stop');
  const btns = el('div', 'btns');
  btns.append(startBtn, stopBtn);
  pop.append(every.wrap, shots.wrap, btns);

  const host = () => row.closest('.leftcol')?.parentElement ?? document.body;

  function place() {
    const h = host();
    if (pop.parentElement !== h) h.append(pop);
    const hr = h.getBoundingClientRect();
    const ar = open.getBoundingClientRect();
    pop.style.left = `${ar.right - hr.left + 10}px`;
    pop.style.top = `${Math.max(0, ar.top - hr.top - 8)}px`;
  }

  function close(refocus = false) {
    if (pop.hidden) return;
    pop.hidden = true;
    open.setAttribute('aria-expanded', 'false');
    if (refocus && open.isConnected) open.focus();
  }

  function show() {
    pop.hidden = false;
    open.setAttribute('aria-expanded', 'true');
    place();
    (store.get().interval?.running ? stopBtn : every.input).focus();
  }

  /** The plan in the fields, written back clamped so the user sees what runs. */
  function readPlan(): IntervalPlan {
    const e = Math.round(Number(every.input.value));
    const everyS = Number.isFinite(e) && every.input.value.trim() !== '' ? Math.max(MIN_EVERY_S, e) : DEFAULT_EVERY_S;
    every.input.value = String(everyS);
    const n = Math.round(Number(shots.input.value));
    const count = shots.input.value.trim() !== '' && Number.isFinite(n) ? Math.max(1, n) : null;
    shots.input.value = count === null ? '' : String(count);
    return { everyS, count };
  }

  open.addEventListener('click', () => (pop.hidden ? show() : close(true)));
  pop.addEventListener('submit', (e) => {
    e.preventDefault();
    if (store.get().interval?.running || startBtn.disabled) return;
    actions.startInterval(readPlan());
    close(true);
  });
  stopBtn.addEventListener('click', () => {
    actions.stopInterval();
    close(true);
  });

  const off = watch(
    store,
    (s) => `${rowValue(s.interval)}|${!!s.camera?.connected}`,
    (_, s) => {
      const running = !!s.interval?.running;
      value.textContent = rowValue(s.interval);
      startBtn.hidden = running;
      stopBtn.hidden = !running;
      startBtn.disabled = !s.camera?.connected;
      every.input.disabled = running;
      shots.input.disabled = running;
    },
  );

  const onKey = (e: KeyboardEvent) => {
    if (e.key !== 'Escape' || pop.hidden) return;
    e.stopPropagation();
    close(true);
  };
  const onDown = (e: PointerEvent) => {
    const t = e.target as Node | null;
    if (pop.hidden || !t || pop.contains(t) || row.contains(t)) return;
    close();
  };
  const onResize = () => !pop.hidden && place();
  document.addEventListener('keydown', onKey, true);
  document.addEventListener('pointerdown', onDown);
  window.addEventListener('resize', onResize);

  return () => {
    off();
    document.removeEventListener('keydown', onKey, true);
    document.removeEventListener('pointerdown', onDown);
    window.removeEventListener('resize', onResize);
    pop.remove();
    row.remove();
  };
}
