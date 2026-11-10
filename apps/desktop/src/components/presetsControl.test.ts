import { beforeEach, describe, expect, test, vi } from 'vitest';
import cameraState from '../fixtures/camera-state.json';
import type { CameraState, Preset } from '../ipc';
import { createActions, type Actions } from '../actions';
import { captureValues } from '../presets';
import { createStore, defaultSettings, initialState, type AppStore } from '../state';
import { presetsControl } from './presetsControl';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => Promise.resolve()) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

const camera = cameraState as unknown as CameraState;
const talking: Preset = {
  id: 'a',
  name: 'Talking head',
  model: 'ZV-E10',
  slot: 1,
  values: [{ code: 0xd21e, raw: { U32: 800 } }],
};

function setup(state: Partial<ReturnType<typeof initialState>> = {}) {
  document.body.innerHTML = '';
  const wrap = document.createElement('div');
  wrap.className = 'dockwrap';
  const glass = document.createElement('div');
  glass.className = 'dock glass';
  wrap.append(glass);
  document.body.append(wrap);
  const settings = { ...defaultSettings('/tmp'), presets: [talking] };
  const store: AppStore = createStore({ ...initialState(), screen: 'main' as const, camera, settings, ...state });
  const actions: Actions = createActions(store);
  vi.spyOn(actions, 'savePreset').mockResolvedValue();
  vi.spyOn(actions, 'deletePreset').mockResolvedValue();
  vi.spyOn(actions, 'applyPreset').mockResolvedValue();
  const control = presetsControl(store, actions);
  glass.append(control.el);
  return { store, actions, control, wrap };
}

const slots = () => [...document.querySelectorAll<HTMLButtonElement>('[role="radio"]')];
const form = () => document.querySelector<HTMLFormElement>('.preset-form');
const menu = () => document.querySelector<HTMLElement>('.preset-menu');

describe('presets control', () => {
  beforeEach(() => vi.restoreAllMocks());

  test('renders four slots, the filled one with its name', () => {
    setup();
    const s = slots();
    expect(s).toHaveLength(4);
    expect(s.map((b) => b.querySelector('small')!.textContent)).toEqual(['C1', 'C2', 'C3', 'C4']);
    expect(s[0].textContent).toBe('C1Talking head');
    expect(s[1].textContent).toBe('C2Empty');
    // The name sits in its own span so long names can be cut short with an ellipsis.
    expect(s[0].querySelector('.pname')!.textContent).toBe('Talking head');
    expect(s[1].querySelector('.pname')!.textContent).toBe('Empty');
    expect(s.every((b) => !b.disabled)).toBe(true);
    expect(document.querySelector('[role="radiogroup"]')!.getAttribute('aria-label')).toBe('Presets');
  });

  test('an empty slot opens the save form; Save stores the current settings', () => {
    const { actions } = setup();
    slots()[1].click();
    const f = form()!;
    expect(f.hidden).toBe(false);
    expect(f.closest('.dockwrap')).not.toBeNull();
    expect(f.querySelector('label')!.textContent).toBe('Save current settings as C2');
    const input = f.querySelector('input')!;
    expect(input.value).toBe('Preset 2');
    expect(f.querySelector('label')!.htmlFor).toBe(input.id);
    expect([...f.querySelectorAll('button')].map((b) => b.textContent)).toEqual(['Save', 'Cancel']);
    input.value = '  Interview ';
    f.querySelector<HTMLButtonElement>('button[type="submit"]')!.click();
    expect(actions.savePreset).toHaveBeenCalledTimes(1);
    const saved = vi.mocked(actions.savePreset).mock.calls[0][0];
    expect(saved).toMatchObject({ name: 'Interview', model: 'ZV-E10', slot: 2, values: captureValues(camera) });
    expect(form()?.hidden ?? true).toBe(true);
  });

  test('Cancel and Escape close the form without saving', () => {
    const { actions } = setup();
    slots()[2].click();
    [...form()!.querySelectorAll('button')].find((b) => b.textContent === 'Cancel')!.click();
    expect(form()?.hidden ?? true).toBe(true);
    slots()[2].click();
    expect(form()!.hidden).toBe(false);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(form()?.hidden ?? true).toBe(true);
    expect(actions.savePreset).not.toHaveBeenCalled();
  });

  test('a filled slot applies its preset', () => {
    const { actions } = setup();
    slots()[0].click();
    expect(actions.applyPreset).toHaveBeenCalledWith(talking);
    expect(form()?.hidden ?? true).toBe(true);
  });

  test('the active slot is checked and the busy one says so', () => {
    const { store } = setup({ activePreset: 1 });
    expect(slots()[0].getAttribute('aria-checked')).toBe('true');
    expect(slots()[0].classList.contains('on')).toBe(true);
    expect(slots()[1].getAttribute('aria-checked')).toBe('false');
    store.set({ presetBusy: 1 });
    expect(slots()[0].getAttribute('aria-busy')).toBe('true');
    expect(slots()[0].classList.contains('busy')).toBe(true);
    store.set({ presetBusy: null });
    expect(slots()[0].hasAttribute('aria-busy')).toBe(false);
    expect(slots()[0].classList.contains('busy')).toBe(false);
  });

  test('the options menu renames, replaces and deletes', () => {
    const { actions } = setup();
    const more = document.querySelector<HTMLButtonElement>('[aria-label="Options for C1"]')!;
    expect(document.querySelectorAll('[aria-label^="Options for"]:not([hidden])')).toHaveLength(1);
    more.click();
    const m = menu()!;
    expect(m.getAttribute('role')).toBe('menu');
    const items = () => [...menu()!.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
    expect(items().map((b) => b.textContent)).toEqual(['Rename', 'Replace with current settings', 'Delete']);

    items()[0].click();
    expect(menu()?.hidden ?? true).toBe(true);
    const f = form()!;
    expect(f.querySelector('input')!.value).toBe('Talking head');
    expect(f.querySelector('button[type="submit"]')!.textContent).toBe('Rename');
    f.querySelector('input')!.value = 'Vlog';
    f.querySelector<HTMLButtonElement>('button[type="submit"]')!.click();
    expect(actions.savePreset).toHaveBeenLastCalledWith({ ...talking, name: 'Vlog' });

    more.click();
    items()[1].click();
    expect(actions.savePreset).toHaveBeenLastCalledWith({ ...talking, values: captureValues(camera) });

    more.click();
    items()[2].click();
    expect(actions.deletePreset).toHaveBeenCalledWith('a');
    expect(menu()?.hidden ?? true).toBe(true);
  });

  test('the menu closes when focus leaves it, not when focus moves inside it', () => {
    setup();
    document.querySelector<HTMLButtonElement>('[aria-label="Options for C1"]')!.click();
    const items = [...menu()!.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
    items[0].dispatchEvent(new FocusEvent('focusout', { bubbles: true, relatedTarget: items[1] }));
    expect(menu()!.hidden).toBe(false);
    items[1].dispatchEvent(new FocusEvent('focusout', { bubbles: true, relatedTarget: document.body }));
    expect(menu()!.hidden).toBe(true);
  });

  test('nothing is clickable without a camera', () => {
    const { actions } = setup({ camera: null });
    expect(slots().every((b) => b.disabled)).toBe(true);
    expect(document.querySelector('[aria-label^="Options for"]:not([hidden])')).toBeNull();
    slots()[0].click();
    slots()[1].click();
    expect(actions.applyPreset).not.toHaveBeenCalled();
    expect(form()?.hidden ?? true).toBe(true);
  });

  test('nothing is clickable when the camera has no usable values', () => {
    setup({ camera: { ...camera, properties: camera.properties.map((p) => ({ ...p, enabled: 'Off' as const })) } });
    expect(slots().every((b) => b.disabled)).toBe(true);
  });

  test('destroy removes the floating form', () => {
    const { control } = setup();
    slots()[1].click();
    control.destroy();
    expect(form()).toBeNull();
  });
});
