// What the camera sets by itself, and how to ask it to.
//
// A camera has no "auto" value for shutter speed or aperture: which of the
// two it chooses is what its shooting mode says. M leaves both to you, A
// (aperture priority) chooses the shutter speed, S the aperture, P both.
// So turning Auto on or off for one of them is a change of mode, within
// the same kind of shooting (photo, movie, slow and quick). ISO is the one
// with an Auto value of its own.

import type { Choice, PropertySummary, PtpValue } from './ipc';
import { modeInfo } from './modes';

export interface AutoNow {
  shutter: boolean;
  aperture: boolean;
}

/** What the camera chooses by itself in the mode it is in; null when the mode is not known. */
export function autoNow(mode: PropertySummary | undefined): AutoNow | null {
  if (!mode) return null;
  switch (modeInfo(mode.raw).badge) {
    case '?': return null;
    case 'M': return { shutter: false, aperture: false };
    case 'A': return { shutter: true, aperture: false };
    case 'S': return { shutter: false, aperture: true };
    // P, the automatic modes and the scenes choose both.
    default: return { shutter: true, aperture: true };
  }
}

/**
 * The mode in which the camera chooses exactly what `want` says, of the same
 * kind of shooting as the mode it is in; null when it lists none. From an
 * automatic mode or a scene the photo modes are meant.
 */
export function modeFor(mode: PropertySummary, want: AutoNow): Choice | null {
  const info = modeInfo(mode.raw);
  const family = info.family === 'scene' ? 'photo' : info.family;
  const letter = want.shutter ? (want.aperture ? 'P' : 'A') : want.aperture ? 'S' : 'M';
  return (
    mode.choices.find((c) => {
      const i = modeInfo(c.raw);
      return i.family === family && i.badge === letter;
    }) ?? null
  );
}

/** Sony's value for ISO chosen by the camera. */
const ISO_AUTO = 0xffffff;
const isoOf = (raw: PtpValue): number | null => ('U32' in raw ? raw.U32 : null);

export const isoIsAuto = (iso: PropertySummary | undefined): boolean => !!iso && isoOf(iso.raw) === ISO_AUTO;

/** The camera's Auto among its ISO values; null when it lists none. */
export const isoAutoChoice = (iso: PropertySummary | undefined): Choice | null =>
  iso?.choices.find((c) => isoOf(c.raw) === ISO_AUTO) ?? null;
