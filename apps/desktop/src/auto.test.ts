import { describe, expect, test } from 'vitest';
import type { PropertySummary } from './ipc';
import { autoNow, isoAutoChoice, isoIsAuto, modeFor } from './auto';

// The real ZV-E10's modes (firmware 2.01), as its report lists them.
const MODES = [
  0x00010002, 0x00020003, 0x00030004, 0x00000001, 0x00078050, 0x00078051, 0x00078052, 0x00078053, 0x00078054,
  0x00098059, 0x0009805a, 0x0009805b, 0x0009805c, 0x00048000, 0x00048001, 0x00058015,
];
const mode = (raw: number, listed = MODES): PropertySummary => ({
  code: 0x500e, label: '', short: '', raw: { U32: raw }, writable: true, enabled: 'On',
  choices: listed.map((m) => ({ raw: { U32: m }, label: '', short: '' })),
});
const to = (from: number, want: { shutter: boolean; aperture: boolean }) => {
  const c = modeFor(mode(from), want);
  return c && 'U32' in c.raw ? c.raw.U32 : null;
};

describe('what the camera chooses by itself', () => {
  test('follows the mode: M neither, A the shutter speed, S the aperture, P and Auto both', () => {
    expect(autoNow(mode(0x00000001))).toEqual({ shutter: false, aperture: false });
    expect(autoNow(mode(0x00020003))).toEqual({ shutter: true, aperture: false });
    expect(autoNow(mode(0x00030004))).toEqual({ shutter: false, aperture: true });
    expect(autoNow(mode(0x00010002))).toEqual({ shutter: true, aperture: true });
    expect(autoNow(mode(0x00048000))).toEqual({ shutter: true, aperture: true });
    expect(autoNow(mode(0x00058015))).toEqual({ shutter: true, aperture: true });
    // The same letters in movie and in slow and quick.
    expect(autoNow(mode(0x00078053))).toEqual({ shutter: false, aperture: false });
    expect(autoNow(mode(0x00078051))).toEqual({ shutter: true, aperture: false });
    expect(autoNow(mode(0x0009805b))).toEqual({ shutter: false, aperture: true });
    expect(autoNow(mode(0x00078054))).toEqual({ shutter: true, aperture: true });
    expect(autoNow(undefined)).toBeNull();
    expect(autoNow(mode(0x00001234))).toBeNull();
  });
});

describe('the mode that makes it so', () => {
  test('from M, automatic shutter speed is A and automatic aperture is S', () => {
    expect(to(0x00000001, { shutter: true, aperture: false })).toBe(0x00020003);
    expect(to(0x00000001, { shutter: false, aperture: true })).toBe(0x00030004);
    expect(to(0x00000001, { shutter: true, aperture: true })).toBe(0x00010002);
  });

  test('and back: from A or S, taking the automatic one in hand is M', () => {
    expect(to(0x00020003, { shutter: false, aperture: false })).toBe(0x00000001);
    expect(to(0x00030004, { shutter: false, aperture: false })).toBe(0x00000001);
    expect(to(0x00010002, { shutter: false, aperture: true })).toBe(0x00030004);
  });

  test('a movie mode stays a movie mode, slow and quick stays slow and quick', () => {
    expect(to(0x00078053, { shutter: true, aperture: false })).toBe(0x00078051);
    expect(to(0x00078054, { shutter: false, aperture: true })).toBe(0x00078052);
    expect(to(0x0009805c, { shutter: true, aperture: true })).toBe(0x00098059);
  });

  test('from Intelligent Auto or a scene the photo modes are meant', () => {
    expect(to(0x00048000, { shutter: false, aperture: true })).toBe(0x00030004);
    expect(to(0x00058015, { shutter: false, aperture: false })).toBe(0x00000001);
  });

  test('is nothing when the camera lists no such mode', () => {
    expect(modeFor(mode(0x00000001, [0x00000001, 0x00010002]), { shutter: true, aperture: false })).toBeNull();
  });
});

describe('ISO', () => {
  const iso = (raw: number): PropertySummary => ({
    code: 0xd21e, label: '', short: '', raw: { U32: raw }, writable: true, enabled: 'On',
    choices: [0xffffff, 100, 125, 160].map((v) => ({ raw: { U32: v }, label: '', short: '' })),
  });
  test('has an Auto of its own', () => {
    expect(isoIsAuto(iso(0xffffff))).toBe(true);
    expect(isoIsAuto(iso(400))).toBe(false);
    expect(isoIsAuto(undefined)).toBe(false);
    expect(isoAutoChoice(iso(400))!.raw).toEqual({ U32: 0xffffff });
    expect(isoAutoChoice({ ...iso(400), choices: [] })).toBeNull();
  });
});
