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
    // Opened from ISO, the Mode row is closed: it shows the mode in use.
    const mode = pop.querySelector<HTMLButtonElement>('.r[data-code="20494"] .pickbtn')!;
    expect(mode.getAttribute('aria-expanded')).toBe('false');
    expect(mode.querySelector('.mn')!.textContent).toBe('Manual Exposure');
    expect(mode.querySelector('.cap')!.textContent).toBe('M');
    expect(pop.querySelector<HTMLElement>('#panel-20494')!.hidden).toBe(true);
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

  test('the Mode chip opens a picker: a tab per kind of shooting, each mode with its icon, name and dial letter', () => {
    const { root } = setup();
    root.querySelectorAll<HTMLButtonElement>('.chip')[0].click();
    const pop = root.querySelector<HTMLElement>('.pop')!;
    const pick = pop.querySelector<HTMLElement>('#panel-20494')!;
    expect(pick.hidden).toBe(false);
    expect(pop.querySelector('.pickbtn')!.getAttribute('aria-expanded')).toBe('true');
    const tabs = () => [...pick.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
    expect(tabs().map((t) => t.textContent)).toEqual(['Photo', 'Movie', 'S&Q', 'Scene']);
    // The camera is in M, a photo mode, so the Photo tab is showing.
    expect(tabs().map((t) => t.getAttribute('aria-selected'))).toEqual(['true', 'false', 'false', 'false']);
    const rows = () =>
      [...pick.querySelectorAll<HTMLButtonElement>('[role="radio"]')].map((b) => [
        b.querySelector('use')!.getAttribute('href'),
        b.querySelector('.mn')!.textContent,
        b.querySelector('.cap')!.textContent,
        b.getAttribute('aria-checked'),
      ]);
    expect(rows()).toEqual([
      ['#m-auto', 'Intelligent Auto', 'AUTO', 'false'],
      ['#m-photo', 'Program Auto', 'P', 'false'],
      ['#m-photo', 'Aperture Priority', 'A', 'false'],
      ['#m-photo', 'Shutter Priority', 'S', 'false'],
      ['#m-photo', 'Manual Exposure', 'M', 'true'],
      ['#m-pano', 'Sweep Panorama', 'PANO', 'false'],
    ]);
    // Another tab shows its modes without changing the camera.
    tabs()[1].click();
    expect(rows()).toEqual([
      ['#m-movie', 'Intelligent Auto', 'AUTO', 'false'],
      ['#m-movie', 'Program Auto', 'P', 'false'],
      ['#m-movie', 'Manual Exposure', 'M', 'false'],
    ]);
    expect(invoke).not.toHaveBeenCalledWith('set_property', expect.anything());
    // Choosing one sets it.
    pick.querySelectorAll<HTMLButtonElement>('[role="radio"]')[1].click();
    expect(invoke).toHaveBeenCalledWith('set_property', { code: 0x500e, raw: { U32: 0x00078050 } });
  });

  test('the picker follows the camera: the mode in use is marked and its tab shown', () => {
    const { root, store } = setup();
    root.querySelectorAll<HTMLButtonElement>('.chip')[0].click();
    const pop = root.querySelector<HTMLElement>('.pop')!;
    const properties = camera.properties.map((p) =>
      p.code === 0x500e ? { ...p, raw: { U32: 0x00058016 }, label: 'Hand-held Twilight', short: 'Twilight' } : p,
    );
    store.set({ camera: { ...camera, properties } });
    expect(pop.querySelector('.pickbtn .mn')!.textContent).toBe('Hand-held Twilight');
    expect(pop.querySelector('.pickbtn .cap')!.textContent).toBe('SCN');
    expect(pop.querySelector('.pickbtn use')!.getAttribute('href')).toBe('#m-twilight');
    expect(pop.querySelector('[role="tab"][aria-selected="true"]')!.textContent).toBe('Scene');
    const checked = pop.querySelector('#panel-20494 [role="radio"][aria-checked="true"] .mn')!;
    expect(checked.textContent).toBe('Hand-held Twilight');
    // The row's own button folds the list away again.
    pop.querySelector<HTMLButtonElement>('.pickbtn')!.click();
    expect(pop.querySelector<HTMLElement>('#panel-20494')!.hidden).toBe(true);
  });

  test('Focus, Drive, White balance and Format open the same kind of list', () => {
    const { root } = setup();
    const chips = root.querySelectorAll<HTMLButtonElement>('.chip');
    const pop = root.querySelector<HTMLElement>('.pop')!;
    const open = (code: number) => pop.querySelector<HTMLElement>(`#panel-${code}`)!;
    const rows = (code: number) =>
      [...open(code).querySelectorAll<HTMLButtonElement>('[role="radio"]')].map((b) => [
        b.querySelector('use')!.getAttribute('href'),
        b.querySelector('.mn')!.textContent,
        b.querySelector('.cap')?.textContent ?? '',
        b.getAttribute('aria-checked'),
      ]);
    // Focus, from its chip: unfolded, the others folded.
    chips[6].click();
    expect(open(0x500a).hidden).toBe(false);
    expect(open(0x500e).hidden).toBe(true);
    expect(rows(0x500a)).toEqual([
      ['#f-af', 'Single-shot AF', 'AF-S', 'false'],
      ['#f-af', 'Automatic AF', 'AF-A', 'false'],
      ['#f-afc', 'Continuous AF', 'AF-C', 'true'],
      ['#f-dmf', 'Direct Manual Focus', 'DMF', 'false'],
      ['#f-mf', 'Manual Focus', 'MF', 'false'],
    ]);
    open(0x500a).querySelectorAll<HTMLButtonElement>('[role="radio"]')[4].click();
    expect(invoke).toHaveBeenCalledWith('set_property', { code: 0x500a, raw: { U16: 1 } });

    // Drive has tabs; unfolding it folds Focus.
    pop.querySelector<HTMLButtonElement>('.r[data-code="20499"] .pickbtn')!.click();
    expect(open(0x500a).hidden).toBe(true);
    expect([...open(0x5013).querySelectorAll('[role="tab"]')].map((t) => t.textContent)).toEqual(['Shoot', 'Timer', 'BRK burst']);
    expect(rows(0x5013)).toEqual([
      ['#d-single', 'Single shooting', '1', 'true'],
      ['#d-burst', 'Continuous shooting: Lo', 'Lo', 'false'],
      ['#d-burst', 'Continuous shooting: Hi', 'Hi', 'false'],
    ]);

    // White balance, and Format under the code this camera keeps it at.
    pop.querySelector<HTMLButtonElement>('.r[data-code="20485"] .pickbtn')!.click();
    expect(rows(0x5005).map((r) => r[1])).toEqual(['Auto', 'Daylight', 'Colour temperature']);
    pop.querySelector<HTMLButtonElement>('.r[data-code="20484"] .pickbtn')!.click();
    expect(rows(0x5004)).toEqual([
      ['#fmt-raw', 'RAW', 'RAW', 'false'],
      ['#fmt-both', 'RAW+JPEG', 'R+J', 'true'],
      ['#fmt-jpeg', 'JPEG', 'JPG', 'false'],
    ]);
    open(0x5004).querySelectorAll<HTMLButtonElement>('[role="radio"]')[0].click();
    expect(invoke).toHaveBeenCalledWith('set_property', { code: 0xd253, raw: { U8: 1 } });
  });

  test('Aperture unfolds the f-stops and asks for the one chosen', () => {
    const { root } = setup();
    root.querySelectorAll<HTMLButtonElement>('.chip')[2].click();
    const panel = root.querySelector<HTMLElement>('#panel-20487')!;
    expect(panel.hidden).toBe(false);
    const stops = [...panel.querySelectorAll<HTMLButtonElement>('.fstop')];
    expect(stops.map((b) => b.textContent)).toContain('f/5.6');
    expect(panel.querySelector('.fstop[aria-checked="true"]')!.textContent).toBe('f/4.0');
    stops.find((b) => b.textContent === 'f/8.0')!.click();
    expect(invoke).toHaveBeenCalledWith('set_property', { code: 0x5007, raw: { U16: 800 } });
    // The row is a list to open, like Drive; single steps are inside it.
    expect(root.querySelectorAll('.r[data-code="20487"] .step button')).toHaveLength(0);
    const nudges = [...panel.querySelectorAll<HTMLButtonElement>('.fnudge button')];
    expect(nudges.map((b) => b.textContent)).toEqual(['One step wider', 'One step narrower']);
    nudges[1].click();
    expect(invoke).toHaveBeenCalledWith('step_property', { code: 0x5007, delta: 1 });
  });

  // The ZV-E10 lists no f-stops as it connects and lists its lens's own a
  // moment later; the row was a plain stepper again whenever it did.
  test('Aperture is a list of the f-stops the camera gives, when it gives them', () => {
    const { root, store } = setup();
    const camera = store.get().camera!;
    const stops = [350, 400, 450, 500, 560, 2200];
    const listed = {
      ...camera,
      properties: camera.properties.map((p) =>
        p.code === 0x5007
          ? {
              ...p,
              raw: { U16: 450 },
              label: 'f/4.5',
              short: 'f/4.5',
              enabled: 'DisplayOnly' as const,
              choices: stops.map((f) => ({ raw: { U16: f }, label: `f/${(f / 100).toFixed(1)}`, short: `f/${(f / 100).toFixed(1)}` })),
            }
          : p,
      ),
    };
    store.set({ camera: listed });
    const chip = root.querySelectorAll<HTMLButtonElement>('.chip')[2];
    // "Display only" with a list is how this camera says a usable aperture.
    expect(chip.classList.contains('off')).toBe(false);
    chip.click();
    const row = root.querySelector<HTMLElement>('.r[data-code="20487"]')!;
    expect(row.querySelector('.pickbtn')).not.toBeNull();
    expect(row.querySelectorAll('.step button')).toHaveLength(0);
    const panel = root.querySelector<HTMLElement>('#panel-20487')!;
    expect(panel.hidden).toBe(false);
    const shown = [...panel.querySelectorAll<HTMLButtonElement>('.fstop')];
    // Only what this lens has, not the whole scale.
    expect(shown.map((b) => b.textContent)).toEqual(['f/3.5', 'f/4.0', 'f/4.5', 'f/5.0', 'f/5.6', 'f/22']);
    expect(panel.querySelector('.fstop[aria-checked="true"]')!.textContent).toBe('f/4.5');
    expect(shown.every((b) => !b.disabled)).toBe(true);
    shown[4].click();
    expect(invoke).toHaveBeenCalledWith('set_property', { code: 0x5007, raw: { U16: 560 } });
  });

  // A tap on the picture makes the camera's focus area a spot; this row is
  // the way back to Wide without going to the camera.
  test('Focus area is a list of the areas the camera gives, named', () => {
    const { root, store } = setup();
    const camera = store.get().camera!;
    const area = (raw: number, label: string, short: string) => ({ raw: { U16: raw }, label, short });
    // What the ZV-E10 listed once settled.
    const listed = [area(1, 'Wide', 'Wide'), area(2, 'Zone', 'Zone'), area(0x103, 'Flexible Spot: L', 'Spot L'), area(0x201, 'Tracking: Wide', 'Trk Wide')];
    store.set({
      camera: {
        ...camera,
        properties: [...camera.properties, { code: 0xd22c, label: 'Flexible Spot: L', short: 'Spot L', raw: { U16: 0x103 }, writable: true, enabled: 'On' as const, choices: listed }],
      },
    });
    root.querySelectorAll<HTMLButtonElement>('.chip')[6].click();
    const pop = root.querySelector<HTMLElement>('.pop')!;
    const row = pop.querySelector<HTMLElement>('.r[data-code="53804"]')!;
    expect(row.querySelector('.k')!.textContent).toBe('Focus area');
    // After Focus, before Drive.
    const order = [...row.parentElement!.querySelectorAll<HTMLElement>('.r')].map((r) => r.querySelector('.k')!.textContent);
    expect(order).toEqual(['Focus', 'Focus area', 'Drive']);
    row.querySelector<HTMLButtonElement>('.pickbtn')!.click();
    const panel = pop.querySelector<HTMLElement>('#panel-53804')!;
    expect([...panel.querySelectorAll('[role="tab"]')].map((t) => t.textContent)).toEqual(['Area', 'Tracking']);
    const names = () => [...panel.querySelectorAll<HTMLButtonElement>('[role="radio"]')].map((b) => [b.querySelector('use')!.getAttribute('href'), b.querySelector('.mn')!.textContent, b.getAttribute('aria-checked')]);
    expect(names()).toEqual([
      ['#fa-wide', 'Wide', 'false'],
      ['#fa-zone', 'Zone', 'false'],
      ['#fa-spot', 'Flexible Spot: L', 'true'],
    ]);
    panel.querySelectorAll<HTMLButtonElement>('[role="radio"]')[0].click();
    expect(invoke).toHaveBeenLastCalledWith('set_property', { code: 0xd22c, raw: { U16: 1 } });
  });

  test('a camera that reports no focus area has no such row', () => {
    const { root } = setup();
    root.querySelectorAll<HTMLButtonElement>('.chip')[6].click();
    expect(root.querySelector('.pop .r[data-code="53804"]')).toBeNull();
  });

  // Roy, 2026-11-11: "add an auto mode" to shutter speed, aperture and ISO.
  // A camera's automatic shutter speed is its aperture priority mode, so
  // the button changes the mode; ISO has an Auto value of its own.
  test('Shutter and Aperture have an Auto button that changes the mode to make it so', () => {
    const { root, store } = setup();
    root.querySelectorAll<HTMLButtonElement>('.chip')[1].click();
    const pop = root.querySelector<HTMLElement>('.pop')!;
    const auto = (code: number) => pop.querySelector<HTMLButtonElement>(`.r[data-code="${code}"] .autobtn`)!;
    const steps = (code: number) => [...pop.querySelectorAll<HTMLButtonElement>(`.r[data-code="${code}"] .step button`)];
    // In M the camera chooses neither.
    expect(auto(0xd20d).textContent).toBe('Auto');
    expect(auto(0xd20d).getAttribute('aria-pressed')).toBe('false');
    expect(auto(0x5007).getAttribute('aria-pressed')).toBe('false');
    expect(steps(0xd20d).every((b) => !b.disabled)).toBe(true);

    // Automatic shutter speed from M is aperture priority.
    auto(0xd20d).click();
    expect(invoke).toHaveBeenLastCalledWith('set_property', { code: 0x500e, raw: { U32: 0x00020003 } });
    const withMode = (raw: number) => ({
      ...store.get().camera!,
      properties: store.get().camera!.properties.map((p) => (p.code === 0x500e ? { ...p, raw: { U32: raw } } : p)),
    });
    store.set({ camera: withMode(0x00020003) });
    expect(auto(0xd20d).getAttribute('aria-pressed')).toBe('true');
    expect(auto(0x5007).getAttribute('aria-pressed')).toBe('false');
    // What the camera chooses is not stepped by hand; the Auto button stays live.
    expect(steps(0xd20d).every((b) => b.disabled)).toBe(true);
    expect(auto(0xd20d).disabled).toBe(false);

    // Aperture too: both automatic is P. Then the shutter taken back in hand: S.
    auto(0x5007).click();
    expect(invoke).toHaveBeenLastCalledWith('set_property', { code: 0x500e, raw: { U32: 0x00010002 } });
    store.set({ camera: withMode(0x00010002) });
    expect(auto(0x5007).getAttribute('aria-pressed')).toBe('true');
    auto(0xd20d).click();
    expect(invoke).toHaveBeenLastCalledWith('set_property', { code: 0x500e, raw: { U32: 0x00030004 } });

    // A movie mode stays a movie mode.
    store.set({ camera: withMode(0x00078053) });
    auto(0xd20d).click();
    expect(invoke).not.toHaveBeenLastCalledWith('set_property', { code: 0x500e, raw: { U32: 0x00020003 } });
  });

  test('ISO has an Auto button: the camera\'s own Auto, and back to the value it had', () => {
    const { root, store } = setup();
    const camera = store.get().camera!;
    const withIso = (raw: number, auto = true) => ({
      ...camera,
      properties: camera.properties.map((p) =>
        p.code === 0xd21e
          ? {
              ...p,
              raw: { U32: raw },
              short: raw === 0xffffff ? 'Auto' : String(raw),
              choices: [...(auto ? [0xffffff] : []), 400, 800, 1600].map((v) => ({ raw: { U32: v }, label: `ISO ${v}`, short: String(v) })),
            }
          : p,
      ),
    });
    store.set({ camera: withIso(800) });
    root.querySelectorAll<HTMLButtonElement>('.chip')[3].click();
    const pop = root.querySelector<HTMLElement>('.pop')!;
    const auto = () => pop.querySelector<HTMLButtonElement>('.r[data-code="53790"]:not(.custom) .autobtn')!;
    expect(auto().getAttribute('aria-pressed')).toBe('false');
    auto().click();
    expect(invoke).toHaveBeenLastCalledWith('set_property', { code: 0xd21e, raw: { U32: 0xffffff } });
    store.set({ camera: withIso(0xffffff) });
    expect(auto().getAttribute('aria-pressed')).toBe('true');
    // Off again goes back to what it was set to before.
    auto().click();
    expect(invoke).toHaveBeenLastCalledWith('set_property', { code: 0xd21e, raw: { U32: 800 } });
    // A camera with no Auto among its ISO values offers none.
    store.set({ camera: withIso(800, false) });
    expect(auto().disabled).toBe(true);
  });

  test('Colour temperature unfolds a scale to drag and a box to type in', () => {
    const { root } = setup();
    root.querySelectorAll<HTMLButtonElement>('.chip')[5].click();
    const pop = root.querySelector<HTMLElement>('.pop')!;
    pop.querySelector<HTMLButtonElement>('.r[data-code="53775"] .pickbtn')!.click();
    const panel = pop.querySelector<HTMLElement>('#panel-53775')!;
    expect(panel.hidden).toBe(false);
    const scale = panel.querySelector<HTMLInputElement>('.kscale')!;
    expect([scale.min, scale.max, scale.step, scale.value]).toEqual(['2500', '9900', '100', '5200']);
    expect(panel.querySelector('.kmin')!.textContent).toBe('2500K');
    expect(panel.querySelector('.kmax')!.textContent).toBe('9900K');
    // Dragging shows the value; letting go sets it.
    scale.value = '6500';
    scale.dispatchEvent(new Event('input', { bubbles: true }));
    expect(panel.querySelector('.know')!.textContent).toBe('6500K');
    expect(invoke).not.toHaveBeenCalledWith('set_property', expect.objectContaining({ code: 0xd20f }));
    scale.dispatchEvent(new Event('change', { bubbles: true }));
    expect(invoke).toHaveBeenCalledWith('set_property', { code: 0xd20f, raw: { U16: 6500 } });
    // A typed value goes to the camera's nearest step, inside its range.
    const input = panel.querySelector<HTMLInputElement>('.ktype input')!;
    input.value = '4321';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(invoke).toHaveBeenCalledWith('set_property', { code: 0xd20f, raw: { U16: 4300 } });
    expect(panel.querySelector('.ktype .range')!.textContent).toBe('Nearest the camera has: 4300');
    input.value = '99999';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(invoke).toHaveBeenCalledWith('set_property', { code: 0xd20f, raw: { U16: 9900 } });
  });

  test('the ISO row takes a typed value, shows the range and sets the nearest stop', () => {
    const { root } = setup();
    root.querySelectorAll<HTMLButtonElement>('.chip')[3].click();
    const pop = root.querySelector<HTMLElement>('.pop')!;
    const input = pop.querySelector<HTMLInputElement>('.r.custom input')!;
    const range = pop.querySelector('.r.custom .range')!;
    expect(input.getAttribute('aria-label')).toBe('Custom ISO');
    expect(range.textContent).toBe('400 to 1600');
    expect([input.min, input.max]).toEqual(['400', '1600']);
    input.value = '1300';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(invoke).toHaveBeenCalledWith('set_property', { code: 0xd21e, raw: { U32: 1600 } });
    // What the camera was sent, since it was not what was typed.
    expect(input.value).toBe('1600');
    expect(range.textContent).toBe('Nearest the camera has: 1600');
    // A value the camera has is set as typed, and the range comes back.
    input.value = '400';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(invoke).toHaveBeenCalledWith('set_property', { code: 0xd21e, raw: { U32: 400 } });
    expect(range.textContent).toBe('400 to 1600');
  });

  test('typing in the ISO box does not step the setting with the arrow keys', () => {
    const { root } = setup();
    root.querySelectorAll<HTMLButtonElement>('.chip')[3].click();
    const input = root.querySelector<HTMLInputElement>('.r.custom input')!;
    input.focus();
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true }));
    expect(invoke).not.toHaveBeenCalledWith('step_property', expect.anything());
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
    const acts = [...root.querySelectorAll<HTMLButtonElement>('.act')].filter((a) => !a.hidden);
    expect(acts.map((a) => a.textContent)).toEqual(['Record', 'Shoot', 'Focus']);
    acts.forEach((a) => a.click());
    expect(invoke).toHaveBeenCalledWith('toggle_record');
    expect(invoke).toHaveBeenCalledWith('shoot');
    expect(invoke).toHaveBeenCalledWith('focus');
  });

  test('Shoot all sits beside Shoot only while more than one camera is connected', () => {
    const { root, store } = setup();
    const labels = () => [...root.querySelectorAll<HTMLButtonElement>('.act')].map((a) => a.textContent);
    const shootAll = () => [...root.querySelectorAll<HTMLButtonElement>('.act')].find((a) => a.textContent === 'Shoot all')!;
    expect(shootAll().hidden).toBe(true);
    store.set({ cameras: { a: camera, b: camera }, activeId: 'a' });
    expect(shootAll().hidden).toBe(false);
    expect(labels()).toEqual(['Record', 'Shoot', 'Shoot all', 'Focus']);
    shootAll().click();
    expect(invoke).toHaveBeenCalledWith('shoot_all');
    store.set({ cameras: { a: camera } });
    expect(shootAll().hidden).toBe(true);
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
