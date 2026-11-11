// What a shooting mode is, for the mode picker: the kind of shooting it
// belongs to, its pictogram, and the letter on its dial cap.
//
// The camera sends a mode as a number. Bodies from about 2020 on (the
// ZV-E10 among them) send 32 bits, the classic 16-bit code with a family
// number above it; the lower 16 bits identify the mode on every body. The
// names come from the Rust side (`sony::decode`), which follows Sony's Help
// Guide; this file only sorts and decorates.

import type { Choice, PtpValue } from './ipc';

export type ModeFamily = 'photo' | 'movie' | 'sq' | 'scene';

/** The tabs of the picker, in order, and what each is called. */
export const FAMILIES: [ModeFamily, string][] = [
  ['photo', 'Photo'],
  ['movie', 'Movie'],
  ['sq', 'S&Q'],
  ['scene', 'Scene'],
];

export interface ModeInfo {
  family: ModeFamily;
  /** The id of the pictogram in the page's sprite. */
  icon: string;
  /** The dial cap: P, A, S, M, AUTO, SCN... */
  badge: string;
}

const UNKNOWN: ModeInfo = { family: 'photo', icon: 'm-photo', badge: '?' };
const LETTERS = ['P', 'A', 'S', 'M', 'AUTO', 'F'];
const SCENES: Record<number, string> = {
  0x0007: 'm-portrait',
  0x8011: 'm-sports',
  0x8012: 'm-sunset',
  0x8013: 'm-night',
  0x8014: 'm-landscape',
  0x8015: 'm-macro',
  0x8016: 'm-twilight',
  0x8017: 'm-nightportrait',
  0x8018: 'm-antiblur',
  0x8019: 'm-scene',
  0x801a: 'm-scene',
  0x801b: 'm-scene',
  0x801c: 'm-scene',
};

function lowWord(raw: PtpValue): number | null {
  const n = 'U32' in raw ? raw.U32 : 'U16' in raw ? raw.U16 : null;
  return n === null ? null : n & 0xffff;
}

export function modeInfo(raw: PtpValue): ModeInfo {
  const code = lowWord(raw);
  if (code === null) return UNKNOWN;
  const photo = (badge: string, icon = 'm-photo'): ModeInfo => ({ family: 'photo', icon, badge });
  switch (code) {
    case 0x0001: return photo('M');
    case 0x0002: return photo('P');
    case 0x0003: return photo('A');
    case 0x0004: return photo('S');
    case 0x8000: return photo('AUTO', 'm-auto');
    case 0x8001: return photo('AUTO+', 'm-auto');
    case 0x8020: return photo('MR');
    case 0x8040:
    case 0x8041: return photo('PANO', 'm-pano');
  }
  // Movie P, A, S, M, Auto, F are consecutive; so are the S&Q ones.
  if (code >= 0x8050 && code <= 0x8055) return { family: 'movie', icon: 'm-movie', badge: LETTERS[code - 0x8050] };
  if (code >= 0x8059 && code <= 0x805e) return { family: 'sq', icon: 'm-sq', badge: LETTERS[code - 0x8059] };
  if (code >= 0x8084 && code <= 0x8087) return { family: 'sq', icon: 'm-sq', badge: LETTERS[code - 0x8084] };
  if (code >= 0x8080 && code <= 0x8083) return { family: 'movie', icon: 'm-sq', badge: LETTERS[code - 0x8080] };
  if (code in SCENES) return { family: 'scene', icon: SCENES[code], badge: 'SCN' };
  return UNKNOWN;
}

/** A mode's name without the " (Movie)" or " (S&Q)" its tab already says. */
export const modeName = (choice: Choice): string => choice.label.replace(/ \((Movie|S&Q|High frame rate)\)$/, '');

/** The kinds of shooting among `choices`, in the picker's order. */
export function familiesOf(choices: Choice[]): ModeFamily[] {
  const present = new Set(choices.map((c) => modeInfo(c.raw).family));
  return FAMILIES.map(([f]) => f).filter((f) => present.has(f));
}

/** Where a dial letter sits in its tab: the automatic mode first, then P, A, S, M as on a dial. */
const RANK = ['AUTO', 'AUTO+', 'P', 'A', 'S', 'M', 'F', 'MR'];

/** The modes of one kind of shooting, automatic first, then P, A, S, M; the rest in the camera's order. */
export function modesIn(choices: Choice[], family: ModeFamily): Choice[] {
  const rank = (c: Choice) => {
    const at = RANK.indexOf(modeInfo(c.raw).badge);
    return at < 0 ? RANK.length : at;
  };
  return choices
    .map((c, i) => ({ c, i }))
    .filter(({ c }) => modeInfo(c.raw).family === family)
    .sort((a, b) => rank(a.c) - rank(b.c) || a.i - b.i)
    .map(({ c }) => c);
}
