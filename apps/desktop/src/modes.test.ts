import { describe, expect, test } from 'vitest';
import type { Choice } from './ipc';
import { familiesOf, modeInfo, modeName } from './modes';

const u32 = (n: number) => ({ U32: n });
const info = (n: number) => modeInfo(u32(n));

describe('shooting modes', () => {
  test('the exposure modes carry their dial letter in each kind of shooting', () => {
    expect(info(0x00010002)).toEqual({ family: 'photo', icon: 'm-photo', badge: 'P' });
    expect(info(0x00020003).badge).toBe('A');
    expect(info(0x00030004).badge).toBe('S');
    expect(info(0x00000001)).toEqual({ family: 'photo', icon: 'm-photo', badge: 'M' });
    expect(info(0x00078050)).toEqual({ family: 'movie', icon: 'm-movie', badge: 'P' });
    expect(info(0x00078053).badge).toBe('M');
    expect(info(0x0009805a)).toEqual({ family: 'sq', icon: 'm-sq', badge: 'A' });
    // Older bodies send the same codes in 16 bits.
    expect(modeInfo({ U16: 0x8050 })).toEqual({ family: 'movie', icon: 'm-movie', badge: 'P' });
  });

  test('the automatic modes, panorama and the scenes', () => {
    expect(info(0x00048000)).toEqual({ family: 'photo', icon: 'm-auto', badge: 'AUTO' });
    expect(info(0x00048001)).toEqual({ family: 'photo', icon: 'm-auto', badge: 'AUTO+' });
    expect(info(0x00078054)).toEqual({ family: 'movie', icon: 'm-movie', badge: 'AUTO' });
    expect(info(0x00068041)).toEqual({ family: 'photo', icon: 'm-pano', badge: 'PANO' });
    expect(info(0x00000007)).toEqual({ family: 'scene', icon: 'm-portrait', badge: 'SCN' });
    expect(info(0x00058011).icon).toBe('m-sports');
    expect(info(0x00058015).icon).toBe('m-macro');
    expect(info(0x00058014).icon).toBe('m-landscape');
    expect(info(0x00058012).icon).toBe('m-sunset');
    expect(info(0x00058013).icon).toBe('m-night');
    expect(info(0x00058016).icon).toBe('m-twilight');
    expect(info(0x00058017).icon).toBe('m-nightportrait');
    expect(info(0x00058018).icon).toBe('m-antiblur');
    for (const scene of [0x58011, 0x58012, 0x58013, 0x58014, 0x58015, 0x58016, 0x58017, 0x58018]) {
      expect(info(scene)).toMatchObject({ family: 'scene', badge: 'SCN' });
    }
  });

  test('a mode Rimix does not know is still a photo mode with a plain mark', () => {
    expect(info(0x000f7777)).toEqual({ family: 'photo', icon: 'm-photo', badge: '?' });
    expect(modeInfo({ Str: 'x' })).toEqual({ family: 'photo', icon: 'm-photo', badge: '?' });
  });

  test('the name in a list leaves out the kind of shooting its tab already says', () => {
    const choice = (label: string): Choice => ({ raw: u32(1), label, short: '' });
    expect(modeName(choice('Program Auto (Movie)'))).toBe('Program Auto');
    expect(modeName(choice('Manual Exposure (S&Q)'))).toBe('Manual Exposure');
    expect(modeName(choice('Hand-held Twilight'))).toBe('Hand-held Twilight');
  });

  test('the kinds of shooting a camera offers, in a fixed order', () => {
    const raws = [0x00078050, 0x00010002, 0x00000007, 0x00098059, 0x00048000];
    expect(familiesOf(raws.map((n) => ({ raw: u32(n), label: '', short: '' })))).toEqual(['photo', 'movie', 'sq', 'scene']);
    expect(familiesOf([{ raw: u32(0x00010002), label: '', short: '' }])).toEqual(['photo']);
  });
});
