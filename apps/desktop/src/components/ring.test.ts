import { beforeEach, describe, expect, test, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import cameraState from '../fixtures/camera-state.json';
import type { CameraState } from '../ipc';
import { CHIP_CODES } from '../props';
import { createStore, initialState, type AppStore } from '../state';
import { createActions } from '../actions';
import { mount } from './ring';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => Promise.resolve()) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

const camera = cameraState as unknown as CameraState;

function setup(): { root: HTMLElement; store: AppStore; unmount: () => void } {
  document.body.innerHTML = '';
  const root = document.createElement('div');
  document.body.append(root);
  const store = createStore({ ...initialState(), screen: 'main' as const, camera });
  const unmount = mount(root, store, createActions(store));
  return { root, store, unmount };
}

const segs = (root: HTMLElement) => [...root.querySelectorAll<SVGGElement>('.ring .seg')];
const text = (g: Element, cls: string) => g.querySelector(`.${cls}`)!.textContent;

describe('ring', () => {
  beforeEach(() => vi.mocked(invoke).mockClear());

  test('ring_renders_ten_segments_bound_to_codes', () => {
    const { root } = setup();
    const all = segs(root);
    expect(all).toHaveLength(10);
    expect(all.slice(0, 9).map((g) => Number(g.dataset.code))).toEqual(CHIP_CODES);
    expect(all.map((g) => [text(g, 'k'), text(g, 'v')])).toEqual([
      ['Mode', 'M'],
      ['Shutter', '1/125'],
      ['Aperture', 'f/4.0'],
      ['ISO', 'ISO 800'],
      ['Exposure comp', '+0.3'],
      ['White balance', '5200K'],
      ['Focus', 'AF-C'],
      ['Drive', 'Single'],
      ['Format', 'RAW+JPEG'],
      ['Picture profile', '—'],
    ]);
    expect(all[9].dataset.code).toBeUndefined();
    expect(all[9].getAttribute('aria-disabled')).toBe('true');
    expect(all[3].getAttribute('aria-label')).toBe('ISO, ISO 800');
  });

  test('segments follow state changes', () => {
    const { root, store } = setup();
    const props = camera.properties.map((p) => (p.code === 0x5007 ? { ...p, short: 'f/5.6' } : p));
    store.set({ camera: { ...camera, properties: props } });
    expect(text(segs(root)[2], 'v')).toBe('f/5.6');
  });

  test('clicking a segment opens the settings panel beside the ring with that row highlighted', () => {
    const { root } = setup();
    const pop = root.querySelector<HTMLElement>('.pop.ringpanel')!;
    expect(pop.hidden).toBe(true);
    const shutter = segs(root)[1];
    shutter.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(pop.hidden).toBe(false);
    expect(shutter.classList.contains('on')).toBe(true);
    expect(shutter.getAttribute('aria-pressed')).toBe('true');
    expect(pop.querySelector('.ttl span')!.textContent).toBe('opened from Shutter');
    expect(pop.querySelector('.r.active')!.getAttribute('data-code')).toBe(String(0xd20d));
    // Another segment moves the highlight; the same one again closes.
    const iso = segs(root)[3];
    iso.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(shutter.classList.contains('on')).toBe(false);
    expect(pop.querySelector('.r.active .k')!.textContent).toBe('ISO');
    iso.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(pop.hidden).toBe(true);
    // The disabled Picture profile segment opens nothing.
    segs(root)[9].dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(pop.hidden).toBe(true);
  });

  test('Done and Escape close the panel; Enter opens it from the keyboard', () => {
    const { root } = setup();
    const pop = root.querySelector<HTMLElement>('.pop.ringpanel')!;
    const mode = segs(root)[0];
    mode.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(pop.hidden).toBe(false);
    pop.querySelector<HTMLButtonElement>('.done')!.click();
    expect(pop.hidden).toBe(true);
    mode.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(pop.hidden).toBe(true);
  });

  test('Shoot in the hub, Record and Focus arcs call their commands', () => {
    const { root } = setup();
    const click = (sel: string) => root.querySelector(sel)!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    click('.ring .shoot');
    click('.ring .act.rec');
    click('.ring .act.af');
    expect(invoke).toHaveBeenCalledWith('shoot');
    expect(invoke).toHaveBeenCalledWith('toggle_record');
    expect(invoke).toHaveBeenCalledWith('focus');
  });

  test('Space on a focused ring control does not reach the shoot shortcut', () => {
    const { root } = setup();
    const onDoc = vi.fn();
    document.addEventListener('keydown', onDoc);
    root.querySelector('.ring .act.rec')!.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    document.removeEventListener('keydown', onDoc);
    expect(invoke).toHaveBeenCalledWith('toggle_record');
    expect(onDoc).not.toHaveBeenCalled();
  });

  test('the presets bar holds the live presets control', () => {
    const { root, unmount } = setup();
    const slots = [...root.querySelectorAll<HTMLButtonElement>('.ringbar [role="radio"]')];
    expect(slots.map((b) => b.querySelector('small')!.textContent)).toEqual(['C1', 'C2', 'C3', 'C4']);
    expect(slots.every((b) => !b.disabled)).toBe(true);
    slots[3].click();
    expect(root.querySelector<HTMLElement>('.preset-form')!.hidden).toBe(false);
    unmount();
    expect(root.querySelector('.preset-form')).toBeNull();
  });

  test('unmount removes the ring, its panel and the presets bar', () => {
    const { root, unmount } = setup();
    unmount();
    expect(root.querySelector('.ring, .ringpanel, .ringbar')).toBeNull();
  });
});
