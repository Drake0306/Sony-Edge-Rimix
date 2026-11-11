// What each setting that is chosen from a list looks like in its picker:
// which tab a value sits under, its pictogram, its name and its badge.
//
// Mode, Drive, Focus, White balance and Format all open the same kind of
// list from the settings popover. The names come from the Rust side
// (`sony::decode`); this file sorts and decorates them.

import type { Choice, PtpValue } from './ipc';
import { FAMILIES, modeInfo, modeName, modesIn } from './modes';

/** How one value shows: a pictogram from the page's sprite, a name, and a short badge (may be empty). */
export interface Look {
  icon: string;
  name: string;
  badge: string;
}

export interface PickerSpec {
  /** What the list is, for assistive technology: "Shooting mode". */
  title: string;
  /** The tabs, in order, as [id, title]. A setting with one group has no tabs. */
  groups: [string, string][];
  groupOf(choice: Choice): string;
  look(choice: Choice): Look;
  /** The values of one group in the order to list them; the camera's order unless given. */
  sort?(choices: Choice[], group: string): Choice[];
}

const u16Of = (raw: PtpValue): number | null => ('U16' in raw ? raw.U16 : 'U32' in raw ? raw.U32 & 0xffff : 'U8' in raw ? raw.U8 : null);

// ---------------------------------------------------------------- mode

const MODE: PickerSpec = {
  title: 'Shooting mode',
  groups: FAMILIES,
  groupOf: (c) => modeInfo(c.raw).family,
  look: (c) => {
    const info = modeInfo(c.raw);
    return { icon: info.icon, name: modeName(c), badge: info.badge };
  },
  sort: (choices, group) => modesIn(choices, group as (typeof FAMILIES)[number][0]),
};

// ---------------------------------------------------------------- drive

/** The tab and look of a drive mode, read from its name ("Self-timer 10 s, 3 shots"). */
export function driveLook(label: string): Look & { group: string } {
  if (label === 'Single') return { group: 'shoot', icon: 'd-single', name: 'Single shooting', badge: '1' };
  const burst = /^Continuous,? ?(.*)$/.exec(label);
  if (burst) return { group: 'shoot', icon: 'd-burst', name: `Continuous shooting${burst[1] ? `: ${burst[1]}` : ''}`, badge: burst[1].replace(', live', '') || 'On' };
  const timer = /^Self-timer (\d+) s(?:, (\d) shots)?$/.exec(label);
  if (timer) {
    const [, secs, shots] = timer;
    return shots
      ? { group: 'timer', icon: 'd-timerburst', name: `${secs} seconds, then ${shots} shots`, badge: `${secs}s×${shots}` }
      : { group: 'timer', icon: 'd-timer', name: `${secs} seconds`, badge: `${secs}s` };
  }
  const bracket = /^Bracket ([\d.]+) EV, (\d) shots( plus| minus)?, (continuous|single)$/.exec(label);
  if (bracket) {
    const [, step, shots, way = '', kind] = bracket;
    return {
      group: kind === 'continuous' ? 'brkc' : 'brks',
      icon: 'd-bracket',
      name: `${step} EV, ${shots} shots${way}`,
      badge: `×${shots}`,
    };
  }
  const level = /^(.* bracket) (Lo|Hi)$/.exec(label);
  if (level) return { group: 'more', icon: 'd-bracket', name: level[1], badge: level[2] };
  return { group: 'more', icon: label.includes('racket') ? 'd-bracket' : 'd-single', name: label, badge: '' };
}

const DRIVE: PickerSpec = {
  title: 'Drive mode',
  groups: [
    ['shoot', 'Shoot'],
    ['timer', 'Timer'],
    ['brkc', 'BRK burst'],
    ['brks', 'BRK single'],
    ['more', 'More'],
  ],
  groupOf: (c) => driveLook(c.label).group,
  look: (c) => driveLook(c.label),
};

// ---------------------------------------------------------------- focus

const FOCUS: PickerSpec = {
  title: 'Focus mode',
  groups: [['all', 'Focus']],
  groupOf: () => 'all',
  look: (c) => {
    const code = u16Of(c.raw);
    const icon = code === 0x0001 ? 'f-mf' : code === 0x8006 ? 'f-dmf' : code === 0x8004 ? 'f-afc' : 'f-af';
    return { icon, name: c.label, badge: c.short };
  },
};

/**
 * Where in the picture autofocus looks. 0x00xx are wide, zone and centre,
 * 0x01xx the spots that can be moved, 0x02xx the tracking form of each.
 */
const FOCUS_AREA: PickerSpec = {
  title: 'Focus area',
  groups: [['area', 'Area'], ['tracking', 'Tracking']],
  groupOf: (c) => ((u16Of(c.raw) ?? 0) >> 8 === 2 ? 'tracking' : 'area'),
  look: (c) => {
    const code = u16Of(c.raw) ?? 0;
    // The tracking forms repeat the list, with the spots from the fourth on.
    const kind = code >> 8 === 1 || (code >> 8 === 2 && (code & 0xff) > 3) ? 'spot' : (code & 0xff) === 1 ? 'wide' : (code & 0xff) === 2 ? 'zone' : 'spot';
    return { icon: `fa-${kind}`, name: c.label, badge: '' };
  },
};

/** Which zooms are allowed past the lens's own: optical only, Clear Image Zoom, digital zoom. */
const ZOOM_SETTING: PickerSpec = {
  title: 'Zoom setting',
  groups: [['all', 'Zoom']],
  groupOf: () => 'all',
  look: (c) => {
    const code = u16Of(c.raw);
    return { icon: code === 1 ? 'z-lens' : 'z-crop', name: c.label, badge: code === 3 ? 'CIZ' : code === 4 ? 'DZ' : '' };
  },
};

/** SteadyShot, on the bodies that let a computer set it. */
const STEADYSHOT: PickerSpec = {
  title: 'SteadyShot',
  groups: [['all', 'SteadyShot']],
  groupOf: () => 'all',
  look: (c) => ({ icon: 'z-steady', name: c.label, badge: '' }),
};

// ---------------------------------------------------------------- white balance

/** White balance raw values meaning "use the colour temperature". */
const WB_KELVIN = [0x8012, 0x8007];
const WB_LOOK: Record<number, [string, string]> = {
  0x0002: ['wb-auto', 'AWB'],
  0x0004: ['wb-sun', ''],
  0x8011: ['wb-shade', ''],
  0x8010: ['wb-cloud', ''],
  0x0006: ['wb-bulb', ''],
  0x8001: ['wb-fluor', '-1'],
  0x8002: ['wb-fluor', '0'],
  0x8003: ['wb-fluor', '+1'],
  0x8004: ['wb-fluor', '+2'],
  0x0007: ['wb-flash', ''],
  0x8030: ['wb-water', ''],
  0x8020: ['wb-custom', '1'],
  0x8021: ['wb-custom', '2'],
  0x8022: ['wb-custom', '3'],
  0x8023: ['wb-custom', ''],
};

const WHITE_BALANCE: PickerSpec = {
  title: 'White balance',
  groups: [['all', 'White balance']],
  groupOf: () => 'all',
  look: (c) => {
    const code = u16Of(c.raw) ?? -1;
    // The kelvin value is its own row; here the choice is the mode itself.
    if (WB_KELVIN.includes(code)) return { icon: 'wb-k', name: 'Colour temperature', badge: 'K' };
    const [icon, badge] = WB_LOOK[code] ?? ['wb-auto', ''];
    return { icon, name: c.label, badge };
  },
};

// ---------------------------------------------------------------- format

const FORMAT: PickerSpec = {
  title: 'File format',
  groups: [['all', 'Format']],
  groupOf: () => 'all',
  look: (c) => {
    const raw = c.label.includes('RAW');
    const other = /JPEG|HEIF/.exec(c.label)?.[0];
    if (raw && other) return { icon: 'fmt-both', name: c.label, badge: other === 'HEIF' ? 'R+H' : 'R+J' };
    if (raw) return { icon: 'fmt-raw', name: c.label, badge: 'RAW' };
    return { icon: 'fmt-jpeg', name: c.label, badge: other === 'HEIF' ? 'HEIF' : 'JPG' };
  },
};

/** Property codes of the settings chosen from a list. Format is 0x5004 on older bodies, 0xD253 on newer. */
const SPECS: Record<number, PickerSpec> = {
  0x500e: MODE,
  0x5013: DRIVE,
  0x500a: FOCUS,
  0xd22c: FOCUS_AREA,
  0xd25f: ZOOM_SETTING,
  0xd0d9: STEADYSHOT,
  0xd0da: STEADYSHOT,
  0x5005: WHITE_BALANCE,
  0x5004: FORMAT,
  0xd253: FORMAT,
};

export const pickerFor = (code: number): PickerSpec | null => SPECS[code] ?? null;

/** The tabs a camera's values fill, in the picker's order. */
export function groupsOf(spec: PickerSpec, choices: Choice[]): [string, string][] {
  const present = new Set(choices.map((c) => spec.groupOf(c)));
  return spec.groups.filter(([id]) => present.has(id));
}

/** The values under one tab, in the order to list them. */
export function choicesIn(spec: PickerSpec, choices: Choice[], group: string): Choice[] {
  return spec.sort ? spec.sort(choices, group) : choices.filter((c) => spec.groupOf(c) === group);
}

/** The f-numbers a lens can have, a third of a stop apart, as the camera counts them (hundredths). */
export const F_STOPS = [
  100, 110, 120, 140, 160, 180, 200, 220, 250, 280, 320, 350, 400, 450, 500, 560, 630, 710, 800, 900, 1000, 1100, 1300,
  1400, 1600, 1800, 2000, 2200,
];

/** "f/3.5", "f/11" from hundredths. */
export const fLabel = (hundredths: number): string =>
  hundredths >= 1000 && hundredths % 100 === 0 ? `f/${hundredths / 100}` : `f/${(hundredths / 100).toFixed(1)}`;

/** The radius, out of 7, of the opening drawn for an f-number: wide open at f/1, a pinhole at f/22. */
export const irisRadius = (hundredths: number): number => Math.max(1, Math.min(6.2, 6.2 / Math.sqrt(hundredths / 100)));
