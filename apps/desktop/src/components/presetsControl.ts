// Preset slots C1 to C4 for the dock and the ring's bar. A filled slot
// applies its preset; an empty one opens a small form that saves the current
// settings into it. Each filled slot has a menu to rename, replace or delete.

import type { Actions } from '../actions';
import { button, el } from '../dom';
import type { Preset } from '../ipc';
import { captureValues, newPreset, presetFor, PRESET_SLOTS } from '../presets';
import type { AppState, AppStore } from '../state';

interface Slot {
  n: number;
  radio: HTMLButtonElement;
  more: HTMLButtonElement;
  name: HTMLSpanElement;
}

let formIds = 0;

export function presetsControl(store: AppStore, actions: Actions): { el: HTMLElement; destroy(): void } {
  const root = el('div', 'presets');
  const group = el('div', 'segc');
  group.setAttribute('role', 'radiogroup');
  group.setAttribute('aria-label', 'Presets');
  root.append(group);

  // ---- the floating form and menu, placed beside the control when opened ----
  const inputId = `preset-name-${++formIds}`;
  const form = el('form', 'preset-form glass primary');
  form.hidden = true;
  const label = el('label');
  label.htmlFor = inputId;
  const input = el('input');
  input.id = inputId;
  input.type = 'text';
  input.maxLength = 40;
  input.autocomplete = 'off';
  input.spellcheck = false;
  const submit = button('pbtn');
  submit.type = 'submit';
  const cancel = button('sbtn', 'Cancel');
  const btns = el('div', 'btns');
  btns.append(submit, cancel);
  form.append(label, input, btns);

  const menu = el('div', 'preset-menu glass primary');
  menu.setAttribute('role', 'menu');
  menu.hidden = true;
  const item = (text: string) => {
    const b = button('', text);
    b.setAttribute('role', 'menuitem');
    b.tabIndex = -1;
    menu.append(b);
    return b;
  };
  const renameItem = item('Rename');
  const replaceItem = item('Replace with current settings');
  const deleteItem = item('Delete');
  const items = [renameItem, replaceItem, deleteItem];

  /** What the open form or menu is for: an empty slot, or a filled one to rename. */
  let target: { slot: number; preset: Preset | null; anchor: HTMLElement } | null = null;

  const host = () => root.closest('.glass')?.parentElement ?? root.parentElement ?? document.body;

  function place(floating: HTMLElement, anchor: HTMLElement) {
    const h = host();
    if (floating.parentElement !== h) h.append(floating);
    const hr = h.getBoundingClientRect();
    const ar = anchor.getBoundingClientRect();
    floating.style.left = `${Math.max(0, ar.left - hr.left)}px`;
    floating.style.bottom = `${hr.bottom - ar.top + 8}px`;
  }

  function close(refocus = false) {
    const anchor = target?.anchor;
    form.hidden = true;
    menu.hidden = true;
    target = null;
    for (const s of slots) s.more.setAttribute('aria-expanded', 'false');
    if (refocus && anchor?.isConnected) anchor.focus();
  }

  function openForm(slot: number, preset: Preset | null, anchor: HTMLElement) {
    close();
    target = { slot, preset, anchor };
    label.textContent = preset ? `Rename C${slot}` : `Save current settings as C${slot}`;
    input.value = preset ? preset.name : `Preset ${slot}`;
    submit.textContent = preset ? 'Rename' : 'Save';
    form.hidden = false;
    place(form, anchor);
    input.focus();
    input.select();
  }

  function openMenu(s: Slot, preset: Preset) {
    close();
    target = { slot: s.n, preset, anchor: s.more };
    menu.setAttribute('aria-label', `Options for C${s.n}`);
    s.more.setAttribute('aria-expanded', 'true');
    menu.hidden = false;
    place(menu, s.more);
    renameItem.focus();
  }

  form.addEventListener('submit', (e) => {
    e.preventDefault();
    const camera = store.get().camera;
    if (!target || !camera) return close();
    const { slot, preset, anchor } = target;
    const name = input.value.trim() || preset?.name || `Preset ${slot}`;
    const next = preset ? { ...preset, name } : newPreset(name, camera.model, slot, captureValues(camera));
    void actions.savePreset(next);
    close();
    if (anchor.isConnected) anchor.focus();
  });
  cancel.addEventListener('click', () => close(true));

  renameItem.addEventListener('click', () => {
    if (!target?.preset) return close();
    const { slot, preset } = target;
    const s = slots[slot - 1];
    openForm(slot, preset, s.radio);
  });
  replaceItem.addEventListener('click', () => {
    const camera = store.get().camera;
    if (target?.preset && camera) void actions.savePreset({ ...target.preset, values: captureValues(camera) });
    close(true);
  });
  deleteItem.addEventListener('click', () => {
    const preset = target?.preset;
    const anchor = target ? slots[target.slot - 1].radio : null;
    close();
    if (preset) void actions.deletePreset(preset.id);
    anchor?.focus();
  });
  // Focus moving to something outside the menu (not its own button) closes it. A null
  // relatedTarget stays open: WebKit does not focus a clicked button, so a click on
  // another item blurs with no target, and outside clicks are handled by pointerdown.
  menu.addEventListener('focusout', (e) => {
    const next = e.relatedTarget as Node | null;
    if (menu.hidden || !next || menu.contains(next) || next === target?.anchor) return;
    close();
  });
  menu.addEventListener('keydown', (e) => {
    const i = items.indexOf(document.activeElement as HTMLButtonElement);
    const step = e.key === 'ArrowDown' ? 1 : e.key === 'ArrowUp' ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    items[(i + step + items.length) % items.length].focus();
  });

  // ---- the slots ----
  const slots: Slot[] = Array.from({ length: PRESET_SLOTS }, (_, i) => {
    const n = i + 1;
    const wrap = el('span', 'pslot');
    const radio = button('');
    radio.setAttribute('role', 'radio');
    const name = el('span', 'pname');
    radio.append(el('small', '', `C${n}`), name);
    const more = button('more', '…', `Options for C${n}`);
    more.setAttribute('aria-haspopup', 'menu');
    more.setAttribute('aria-expanded', 'false');
    wrap.append(radio, more);
    group.append(wrap);
    const slot = { n, radio, more, name };
    radio.addEventListener('click', () => {
      const { camera, settings, presetBusy } = store.get();
      if (!camera || radio.disabled || presetBusy !== null) return;
      const preset = presetFor(settings, camera.model, n);
      if (preset) {
        close();
        void actions.applyPreset(preset);
      } else if (target?.slot === n && !form.hidden) close(true);
      else openForm(n, null, radio);
    });
    more.addEventListener('click', () => {
      const { camera, settings } = store.get();
      const preset = presetFor(settings, camera?.model, n);
      if (!preset || more.disabled) return;
      if (target?.anchor === more && !menu.hidden) close(true);
      else openMenu(slot, preset);
    });
    return slot;
  });

  function render(state: AppState) {
    const { camera, settings, presetBusy, activePreset } = state;
    const usable = !!camera && captureValues(camera).length > 0;
    for (const s of slots) {
      const preset = presetFor(settings, camera?.model, s.n);
      s.name.textContent = preset ? preset.name : 'Empty';
      s.radio.classList.toggle('empty', !preset);
      s.radio.disabled = !usable;
      s.radio.title = preset ? `Apply ${preset.name}` : `Save current settings as C${s.n}`;
      const active = !!preset && activePreset === s.n;
      s.radio.setAttribute('aria-checked', String(active));
      s.radio.classList.toggle('on', active);
      const busy = !!preset && presetBusy === s.n;
      s.radio.classList.toggle('busy', busy);
      if (busy) s.radio.setAttribute('aria-busy', 'true');
      else s.radio.removeAttribute('aria-busy');
      s.more.hidden = !preset || !usable;
      s.more.disabled = !preset || !usable;
    }
    // A form or menu whose slot changed under it no longer makes sense.
    if (target) {
      const now = presetFor(settings, camera?.model, target.slot) ?? null;
      if (!usable || now?.id !== target.preset?.id) close();
    }
  }

  let last: unknown[] = [];
  const pick = (s: AppState) => [s.camera, s.settings, s.presetBusy, s.activePreset];
  const update = (s: AppState) => {
    const next = pick(s);
    if (next.every((v, i) => v === last[i])) return;
    last = next;
    render(s);
  };
  update(store.get());
  const off = store.subscribe(update);

  const onKey = (e: KeyboardEvent) => {
    if (e.key !== 'Escape' || !target) return;
    e.stopPropagation();
    close(true);
  };
  const onDown = (e: PointerEvent) => {
    const t = e.target as Node | null;
    if (!target || !t || form.contains(t) || menu.contains(t) || root.contains(t)) return;
    close();
  };
  document.addEventListener('keydown', onKey, true);
  document.addEventListener('pointerdown', onDown);

  return {
    el: root,
    destroy() {
      off();
      document.removeEventListener('keydown', onKey, true);
      document.removeEventListener('pointerdown', onDown);
      form.remove();
      menu.remove();
    },
  };
}
