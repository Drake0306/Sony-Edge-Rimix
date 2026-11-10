import { describe, expect, test, vi } from 'vitest';
import type { Actions } from './actions';
import { handleShortcut } from './app';
import { createStore, initialState, type AppState } from './state';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => Promise.resolve()) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

function setup() {
  const store = createStore<AppState>({ ...initialState(), screen: 'main' });
  const actions = { shoot: vi.fn(), toggleRecord: vi.fn(), focus: vi.fn() } as unknown as Actions;
  const press = (key: string, init: KeyboardEventInit = {}, target: EventTarget = document.body) => {
    const e = new KeyboardEvent('keydown', { key, cancelable: true, ...init });
    Object.defineProperty(e, 'target', { value: target });
    handleShortcut(e, store, actions);
    return e;
  };
  return { store, actions, press };
}

describe('shortcuts', () => {
  test('Space on a focused button presses the button, not the shutter', () => {
    const { actions, press } = setup();
    const stop = document.createElement('button');
    const e = press(' ', {}, stop);
    expect(actions.shoot).not.toHaveBeenCalled();
    expect(e.defaultPrevented).toBe(false);
    press('f', {}, stop);
    expect(actions.focus).toHaveBeenCalledTimes(1);
  });

  test('Space, R and F shoot, record and focus', () => {
    const { actions, press } = setup();
    expect(press(' ').defaultPrevented).toBe(true);
    press('r');
    press('F');
    expect(actions.shoot).toHaveBeenCalledTimes(1);
    expect(actions.toggleRecord).toHaveBeenCalledTimes(1);
    expect(actions.focus).toHaveBeenCalledTimes(1);
  });

  test('held keys do not repeat shots or flip recording', () => {
    const { actions, press } = setup();
    press(' ', { repeat: true });
    press('r', { repeat: true });
    expect(actions.shoot).not.toHaveBeenCalled();
    expect(actions.toggleRecord).not.toHaveBeenCalled();
  });

  test('typing in a field and the connect screen are left alone', () => {
    const { store, actions, press } = setup();
    press(' ', {}, document.createElement('input'));
    store.set({ screen: 'connect' });
    press(' ');
    expect(actions.shoot).not.toHaveBeenCalled();
  });
});
