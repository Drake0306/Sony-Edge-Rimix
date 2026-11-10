import { beforeEach, describe, expect, test, vi } from 'vitest';
import cameraState from '../fixtures/camera-state.json';
import type { Actions } from '../actions';
import type { CameraState } from '../ipc';
import { start } from '../interval';
import { createStore, initialState, type AppState, type AppStore } from '../state';
import { mount, rowValue } from './intervalTimer';

const camera = { ...(cameraState as unknown as CameraState), connected: true };

function setup(state: Partial<AppState> = {}) {
  document.body.innerHTML = '';
  const host = document.createElement('div');
  const group = document.createElement('div');
  host.append(group);
  document.body.append(host);
  const store: AppStore = createStore<AppState>({ ...initialState(), screen: 'main', camera, ...state });
  const actions = { startInterval: vi.fn(), stopInterval: vi.fn() } as unknown as Actions;
  const unmount = mount(group, store, actions);
  const row = group.querySelector<HTMLButtonElement>('.r.row-btn > button')!;
  const pop = () => document.querySelector<HTMLElement>('.interval-pop')!;
  const field = (name: string) => pop().querySelector<HTMLInputElement>(`input[aria-label="${name}"]`)!;
  const btn = (text: string) => [...pop().querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent === text)!;
  return { store, actions, unmount, row, pop, field, btn };
}

describe('interval timer', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
  });

  test('the row reads Off and opens the popover with its defaults', () => {
    const { row, pop, field, btn } = setup();
    expect(row.querySelector('.k')!.textContent).toBe('Interval timer');
    expect(row.querySelector('.v')!.textContent).toBe('Off');
    expect(row.getAttribute('aria-expanded')).toBe('false');
    row.click();
    expect(pop().hidden).toBe(false);
    expect(row.getAttribute('aria-expanded')).toBe('true');
    expect(field('Every').value).toBe('5');
    expect(field('Every').min).toBe('2');
    expect(field('Shots').value).toBe('');
    expect(btn('Start').hidden).toBe(false);
    expect(btn('Stop').hidden).toBe(true);
  });

  test('Start calls startInterval with the plan and closes', () => {
    const { row, pop, field, btn, actions } = setup();
    row.click();
    field('Shots').value = '20';
    btn('Start').click();
    expect(actions.startInterval).toHaveBeenCalledWith({ everyS: 5, count: 20 });
    expect(pop().hidden).toBe(true);
  });

  test('Every below 2 is clamped and empty Shots is unlimited', () => {
    const { row, field, btn, actions } = setup();
    row.click();
    field('Every').value = '1';
    btn('Start').click();
    expect(actions.startInterval).toHaveBeenCalledWith({ everyS: 2, count: null });
    expect(field('Every').value).toBe('2');
  });

  test('while running the row shows the plan and Stop clears it', () => {
    const { row, store, btn, actions } = setup();
    store.set({ interval: start({ everyS: 5, count: null }, 0) });
    expect(row.querySelector('.v')!.textContent).toBe('Every 5 s');
    store.set({ interval: start({ everyS: 5, count: 20 }, 0) });
    expect(row.querySelector('.v')!.textContent).toBe('Every 5 s, 20 shots');
    row.click();
    expect(btn('Start').hidden).toBe(true);
    expect(btn('Stop').hidden).toBe(false);
    btn('Stop').click();
    expect(actions.stopInterval).toHaveBeenCalled();
    store.set({ interval: null });
    expect(row.querySelector('.v')!.textContent).toBe('Off');
  });

  test('Escape closes the popover and returns focus to the row', () => {
    const { row, pop } = setup();
    row.click();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(pop().hidden).toBe(true);
    expect(document.activeElement).toBe(row);
  });

  test('rowValue', () => {
    expect(rowValue(null)).toBe('Off');
    expect(rowValue(start({ everyS: 3, count: 1 }, 0))).toBe('Every 3 s, 1 shot');
  });

  test('unmount removes the row and the popover', () => {
    const { row, unmount } = setup();
    row.click();
    unmount();
    expect(document.querySelector('.interval-pop')).toBeNull();
    expect(row.isConnected).toBe(false);
  });
});
