import { beforeEach, describe, expect, test, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import cameraState from '../fixtures/camera-state.json';
import type { CameraState } from '../ipc';
import { createStore, initialState, type AppStore } from '../state';
import { createActions } from '../actions';
import { start } from '../interval';
import { liveStatusText, mount } from './dock';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => Promise.resolve()) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

const camera = cameraState as unknown as CameraState;

const unmounts: Array<() => void> = [];

function setup(): { root: HTMLElement; store: AppStore } {
  document.body.innerHTML = '';
  const root = document.createElement('div');
  document.body.append(root);
  const store = createStore({ ...initialState(), screen: 'main' as const, camera });
  unmounts.push(mount(root, store, createActions(store)));
  return { root, store };
}

describe('dock', () => {
  beforeEach(() => {
    unmounts.splice(0).forEach((u) => u());
    vi.mocked(invoke).mockClear();
  });

  test('dock_renders_ten_chips_with_short_labels', () => {
    const { root } = setup();
    const chips = [...root.querySelectorAll<HTMLButtonElement>('.chip')];
    expect(chips).toHaveLength(10);
    const pairs = chips.map((c) => [c.querySelector('small')!.textContent, c.querySelector('b')!.textContent]);
    expect(pairs).toEqual([
      ['Mode', 'M'],
      ['Shutter', '1/125'],
      ['Aperture', 'f/4.0'],
      ['ISO', '800'],
      ['Exposure comp', '+0.3'],
      // White balance shows the colour temperature when set to it.
      ['White balance', '5200K'],
      ['Focus', 'AF-C'],
      ['Drive', 'Single'],
      ['Format', 'RAW+JPEG'],
      ['Picture profile', '—'],
    ]);
    expect(chips[9].disabled).toBe(true);
    expect(chips[9].title).toBe('Not available yet');
  });

  test('chips follow state changes and look disabled when the camera disables them', () => {
    const { root, store } = setup();
    const props = camera.properties.map((p) =>
      p.code === 0xd21e ? { ...p, short: '1600', enabled: 'Off' as const } : p,
    );
    store.set({ camera: { ...camera, properties: props } });
    const iso = root.querySelectorAll<HTMLButtonElement>('.chip')[3];
    expect(iso.querySelector('b')!.textContent).toBe('1600');
    expect(iso.classList.contains('off')).toBe(true);
    // A bare kelvin number from the colour temp property gets its unit.
    const kelvin = props.map((p) => (p.code === 0xd20f ? { ...p, short: '5600' } : p));
    store.set({ camera: { ...camera, properties: kelvin } });
    expect(root.querySelectorAll('.chip')[5].querySelector('b')!.textContent).toBe('5600K');
  });

  test('clicking a chip opens the settings popover anchored to it', () => {
    const { root } = setup();
    const pop = root.querySelector<HTMLElement>('.pop')!;
    expect(pop.hidden).toBe(true);
    const iso = root.querySelectorAll<HTMLButtonElement>('.chip')[3];
    iso.click();
    expect(pop.hidden).toBe(false);
    expect(iso.classList.contains('open')).toBe(true);
    expect(iso.getAttribute('aria-expanded')).toBe('true');
    expect(pop.querySelector('.ttl span')!.textContent).toBe('opened from ISO');
    expect(pop.querySelector('.r.active .k')!.textContent).toBe('ISO');
    // Mode has four choices, so it is a segmented control.
    expect([...pop.querySelectorAll('.r[data-code="20494"] .mini button')].map((b) => b.textContent)).toEqual([
      'P', 'A', 'S', 'M',
    ]);
    // White balance is colour temp, so the colour temp row appears.
    expect(pop.querySelector('.r[data-code="53775"] .k')!.textContent).toBe('Colour temp');
    iso.click();
    expect(pop.hidden).toBe(true);
  });

  test('stepper_calls_step_property_with_minus_one', () => {
    const { root } = setup();
    root.querySelectorAll<HTMLButtonElement>('.chip')[1].click();
    const row = root.querySelector<HTMLElement>('.pop .r[data-code="53773"]')!;
    row.querySelector<HTMLButtonElement>('.step button')!.click();
    expect(invoke).toHaveBeenCalledWith('step_property', { code: 0xd20d, delta: -1 });
    expect(row.classList.contains('pending')).toBe(true);
    row.querySelectorAll<HTMLButtonElement>('.step button')[1].click();
    expect(invoke).toHaveBeenCalledWith('step_property', { code: 0xd20d, delta: 1 });
  });

  test('arrow keys step the open chip and nothing when closed', () => {
    const { root } = setup();
    const press = (key: string, init: KeyboardEventInit = {}) =>
      document.dispatchEvent(new KeyboardEvent('keydown', { key, cancelable: true, ...init }));
    const steps = () => vi.mocked(invoke).mock.calls.filter((c) => c[0] === 'step_property');
    press('ArrowRight');
    expect(steps()).toHaveLength(0);
    root.querySelectorAll<HTMLButtonElement>('.chip')[1].click();
    press('ArrowRight');
    expect(invoke).toHaveBeenLastCalledWith('step_property', { code: 0xd20d, delta: 1 });
    press('ArrowLeft');
    expect(invoke).toHaveBeenLastCalledWith('step_property', { code: 0xd20d, delta: -1 });
    press('ArrowUp');
    expect(invoke).toHaveBeenLastCalledWith('step_property', { code: 0xd20d, delta: 1 });
    press('ArrowDown');
    expect(invoke).toHaveBeenLastCalledWith('step_property', { code: 0xd20d, delta: -1 });
    expect(steps()).toHaveLength(4);
    press('ArrowRight', { metaKey: true });
    expect(steps()).toHaveLength(4);
    press('Escape');
    press('ArrowRight');
    expect(steps()).toHaveLength(4);
  });

  test('arrow keys do nothing while typing', () => {
    const { root } = setup();
    root.querySelectorAll<HTMLButtonElement>('.chip')[1].click();
    const input = document.createElement('input');
    document.body.append(input);
    input.focus();
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    expect(vi.mocked(invoke).mock.calls.filter((c) => c[0] === 'step_property')).toHaveLength(0);
  });

  test('arrow keys already handled, or pressed inside a menu, step nothing', () => {
    const { root } = setup();
    root.querySelectorAll<HTMLButtonElement>('.chip')[1].click();
    const steps = () => vi.mocked(invoke).mock.calls.filter((c) => c[0] === 'step_property');
    const handled = new KeyboardEvent('keydown', { key: 'ArrowRight', cancelable: true });
    handled.preventDefault();
    document.dispatchEvent(handled);
    expect(steps()).toHaveLength(0);

    const menu = document.createElement('div');
    menu.setAttribute('role', 'menu');
    const item = document.createElement('button');
    menu.append(item);
    document.body.append(menu);
    item.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }));
    expect(steps()).toHaveLength(0);
  });

  test('Escape closes the popover', () => {
    const { root } = setup();
    root.querySelectorAll<HTMLButtonElement>('.chip')[0].click();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(root.querySelector<HTMLElement>('.pop')!.hidden).toBe(true);
  });

  test('the presets control has four live slots and its form opens inside the dock', () => {
    const { root } = setup();
    const slots = [...root.querySelectorAll<HTMLButtonElement>('.dock [role="radiogroup"] [role="radio"]')];
    expect(slots.map((b) => b.querySelector('small')!.textContent)).toEqual(['C1', 'C2', 'C3', 'C4']);
    expect(slots.some((b) => b.hasAttribute('aria-disabled') || b.disabled)).toBe(false);
    expect(root.querySelector('[title="Presets arrive in the next version"]')).toBeNull();
    slots[0].click();
    const form = root.querySelector<HTMLElement>('.dockwrap > .preset-form')!;
    expect(form.hidden).toBe(false);
    expect(form.querySelector('label')!.textContent).toBe('Save current settings as C1');
  });

  test('Shoot, Record and Focus call their commands', () => {
    const { root } = setup();
    const acts = [...root.querySelectorAll<HTMLButtonElement>('.act')];
    expect(acts.map((a) => a.textContent)).toEqual(['Record', 'Shoot', 'Focus']);
    acts.forEach((a) => a.click());
    expect(invoke).toHaveBeenCalledWith('toggle_record');
    expect(invoke).toHaveBeenCalledWith('shoot');
    expect(invoke).toHaveBeenCalledWith('focus');
  });
});

describe('dock status', () => {
  test('shows the interval while it runs, else the live view', () => {
    const base = { ...initialState(), status: { fps: 24, lastShotAt: null } };
    expect(liveStatusText(base, 0)).toBe('Live view 24 fps');
    const interval = { ...start({ everyS: 5, count: 20 }, 0), taken: 3 };
    expect(liveStatusText({ ...base, interval }, 1000)).toBe('Interval 3 of 20, next in 4 s');
  });

  test('the status line follows a running interval', () => {
    const { root, store } = setup();
    store.set({ interval: start({ everyS: 5, count: null }, Date.now()) });
    expect(root.querySelector('.dockrow .right')!.textContent).toMatch(/^Interval 0 taken, next in [45] s$/);
    store.set({ interval: null });
    expect(root.querySelector('.dockrow .right')!.textContent).toMatch(/^Live view/);
  });
});
