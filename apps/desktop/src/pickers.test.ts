import { describe, expect, test } from 'vitest';
import type { Choice } from './ipc';
import { choicesIn, driveLook, fLabel, F_STOPS, groupsOf, irisRadius, pickerFor } from './pickers';

const choice = (raw: Choice['raw'], label: string, short = label): Choice => ({ raw, label, short });

describe('drive modes', () => {
  test('single and continuous shooting', () => {
    expect(driveLook('Single')).toEqual({ group: 'shoot', icon: 'd-single', name: 'Single shooting', badge: '1' });
    expect(driveLook('Continuous Lo')).toEqual({ group: 'shoot', icon: 'd-burst', name: 'Continuous shooting: Lo', badge: 'Lo' });
    expect(driveLook('Continuous Hi+').badge).toBe('Hi+');
    expect(driveLook('Continuous Mid').badge).toBe('Mid');
  });

  test('self-timers, with and without a burst', () => {
    expect(driveLook('Self-timer 10 s')).toEqual({ group: 'timer', icon: 'd-timer', name: '10 seconds', badge: '10s' });
    expect(driveLook('Self-timer 5 s, 3 shots')).toEqual({
      group: 'timer',
      icon: 'd-timerburst',
      name: '5 seconds, then 3 shots',
      badge: '5s×3',
    });
  });

  test('brackets go under burst or single, and the others under More', () => {
    expect(driveLook('Bracket 0.3 EV, 3 shots, continuous')).toEqual({
      group: 'brkc',
      icon: 'd-bracket',
      name: '0.3 EV, 3 shots',
      badge: '×3',
    });
    expect(driveLook('Bracket 2.0 EV, 5 shots, single')).toMatchObject({ group: 'brks', name: '2.0 EV, 5 shots', badge: '×5' });
    expect(driveLook('White balance bracket Hi')).toEqual({ group: 'more', icon: 'd-bracket', name: 'White balance bracket', badge: 'Hi' });
    expect(driveLook('DRO bracket Lo')).toMatchObject({ group: 'more', badge: 'Lo' });
    expect(driveLook('Unknown (0x1234)')).toEqual({ group: 'more', icon: 'd-single', name: 'Unknown (0x1234)', badge: '' });
  });

  test('only the tabs the camera has values for are shown, in order', () => {
    const spec = pickerFor(0x5013)!;
    const choices = ['Bracket 0.3 EV, 3 shots, single', 'Single', 'Self-timer 2 s', 'Continuous Hi'].map((l) => choice({ U32: 1 }, l));
    expect(groupsOf(spec, choices).map(([, title]) => title)).toEqual(['Shoot', 'Timer', 'BRK single']);
    expect(choicesIn(spec, choices, 'shoot').map((c) => c.label)).toEqual(['Single', 'Continuous Hi']);
  });
});

describe('focus, white balance and format', () => {
  test('focus modes show Sony\'s name and the short form as the badge', () => {
    const spec = pickerFor(0x500a)!;
    expect(spec.look(choice({ U16: 0x8004 }, 'Continuous AF', 'AF-C'))).toEqual({ icon: 'f-afc', name: 'Continuous AF', badge: 'AF-C' });
    expect(spec.look(choice({ U16: 2 }, 'Single-shot AF', 'AF-S')).icon).toBe('f-af');
    expect(spec.look(choice({ U16: 0x8006 }, 'Direct Manual Focus', 'DMF')).icon).toBe('f-dmf');
    expect(spec.look(choice({ U16: 1 }, 'Manual Focus', 'MF')).icon).toBe('f-mf');
    expect(groupsOf(spec, [choice({ U16: 1 }, 'Manual Focus', 'MF')])).toHaveLength(1);
  });

  test('white balance: an icon each, and colour temperature named as the mode it is', () => {
    const spec = pickerFor(0x5005)!;
    expect(spec.look(choice({ U16: 2 }, 'Auto'))).toEqual({ icon: 'wb-auto', name: 'Auto', badge: 'AWB' });
    expect(spec.look(choice({ U16: 4 }, 'Daylight')).icon).toBe('wb-sun');
    expect(spec.look(choice({ U16: 0x8011 }, 'Shade')).icon).toBe('wb-shade');
    expect(spec.look(choice({ U16: 0x8010 }, 'Cloudy')).icon).toBe('wb-cloud');
    expect(spec.look(choice({ U16: 6 }, 'Incandescent')).icon).toBe('wb-bulb');
    expect(spec.look(choice({ U16: 0x8003 }, 'Fluorescent day white'))).toMatchObject({ icon: 'wb-fluor', badge: '+1' });
    expect(spec.look(choice({ U16: 7 }, 'Flash')).icon).toBe('wb-flash');
    expect(spec.look(choice({ U16: 0x8030 }, 'Underwater auto')).icon).toBe('wb-water');
    // Its label is the kelvin value while it is in use; the list names the mode.
    expect(spec.look(choice({ U16: 0x8012 }, '5500K'))).toEqual({ icon: 'wb-k', name: 'Colour temperature', badge: 'K' });
    expect(spec.look(choice({ U16: 0x8021 }, 'Custom 2'))).toEqual({ icon: 'wb-custom', name: 'Custom 2', badge: '2' });
  });

  test('file format, under either property code', () => {
    for (const code of [0x5004, 0xd253]) {
      const spec = pickerFor(code)!;
      expect(spec.look(choice({ U8: 1 }, 'RAW'))).toEqual({ icon: 'fmt-raw', name: 'RAW', badge: 'RAW' });
      expect(spec.look(choice({ U8: 2 }, 'RAW+JPEG'))).toEqual({ icon: 'fmt-both', name: 'RAW+JPEG', badge: 'R+J' });
      expect(spec.look(choice({ U8: 3 }, 'JPEG'))).toEqual({ icon: 'fmt-jpeg', name: 'JPEG', badge: 'JPG' });
    }
    expect(pickerFor(0xd21e)).toBeNull();
  });
});

describe('f-stops', () => {
  test('are named as a lens ring names them', () => {
    expect(fLabel(350)).toBe('f/3.5');
    expect(fLabel(400)).toBe('f/4.0');
    expect(fLabel(1100)).toBe('f/11');
    expect(fLabel(2200)).toBe('f/22');
    expect(F_STOPS).toContain(560);
  });

  test('a wider aperture is drawn with a wider opening', () => {
    expect(irisRadius(140)).toBeGreaterThan(irisRadius(400));
    expect(irisRadius(400)).toBeGreaterThan(irisRadius(1600));
    expect(irisRadius(100)).toBeLessThanOrEqual(6.2);
    expect(irisRadius(6400)).toBeGreaterThanOrEqual(1);
  });
});
