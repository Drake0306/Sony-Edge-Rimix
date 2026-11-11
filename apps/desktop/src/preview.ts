// A browser preview of the app with a pretend camera behind it, for working
// on the interface without the Tauri shell or a camera:
//
//   pnpm dev, then open http://localhost:1420/preview.html
//
// Query parameters: layout=Dock|Ring, theme=Light|Dark, presets=<count>,
// mode=<raw exposure mode>, drive=<raw drive mode>, apply=<slot to apply>, open=<chip name>, transfers=1 (files arriving), recording=1,
// screen=connect, webcam=1, copying=1 (copying after each shot shown), copy=0 (its switch off), card=1 (the card
// browser), settings=1 (App settings), openas=Remote|Webcam|Files, report=<file of docs/camera-reports>.
// Never part of the built app.

/// <reference types="vite/client" />
import { mockIPC } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import cameraState from './fixtures/camera-state.json';
import type { AppSettings, CameraState, Preset, PropertySummary, PtpValue } from './ipc';
import { defaultSettings } from './state';

const q = new URLSearchParams(location.search);
const CAMERA_ID = '192.168.122.1:15740';
const num = (key: string, fallback: number) => (q.has(key) ? Number(q.get(key)) : fallback);

const u32 = (n: number): PtpValue => ({ U32: n });

// The real ZV-E10's lists (firmware 2.01), as its camera report gave them.
const MODES: [number, string, string][] = [
  [0x00010002, 'Program Auto', 'P'], [0x00020003, 'Aperture Priority', 'A'], [0x00030004, 'Shutter Priority', 'S'],
  [0x00000001, 'Manual Exposure', 'M'], [0x00078050, 'Program Auto (Movie)', 'Movie P'],
  [0x00078051, 'Aperture Priority (Movie)', 'Movie A'], [0x00078052, 'Shutter Priority (Movie)', 'Movie S'],
  [0x00078053, 'Manual Exposure (Movie)', 'Movie M'], [0x00078054, 'Intelligent Auto (Movie)', 'Movie Auto'],
  [0x00098059, 'Program Auto (S&Q)', 'S&Q P'], [0x0009805a, 'Aperture Priority (S&Q)', 'S&Q A'],
  [0x0009805b, 'Shutter Priority (S&Q)', 'S&Q S'], [0x0009805c, 'Manual Exposure (S&Q)', 'S&Q M'],
  [0x00048000, 'Intelligent Auto', 'Auto'], [0x00048001, 'Superior Auto', 'Auto+'], [0x00068041, 'Sweep Panorama', 'Panorama'],
  [0x00000007, 'Portrait', 'Portrait'], [0x00058011, 'Sports Action', 'Sports'], [0x00058015, 'Macro', 'Macro'],
  [0x00058014, 'Landscape', 'Landscape'], [0x00058012, 'Sunset', 'Sunset'], [0x00058013, 'Night Scene', 'Night scene'],
  [0x00058016, 'Hand-held Twilight', 'Twilight'], [0x00058017, 'Night Portrait', 'Night portrait'],
  [0x00058018, 'Anti Motion Blur', 'Anti blur'],
];
const DRIVES: [number, string, string][] = [
  [0x00000001, 'Single', 'Single'], [0x00018012, 'Continuous Lo', 'Cont. Lo'], [0x00018015, 'Continuous Mid', 'Cont. Mid'],
  [0x00010002, 'Continuous Hi', 'Cont. Hi'], [0x00018010, 'Continuous Hi+', 'Cont. Hi+'], [0x00038004, 'Self-timer 10 s', 'Timer 10 s'],
  [0x00038003, 'Self-timer 5 s', 'Timer 5 s'], [0x00038005, 'Self-timer 2 s', 'Timer 2 s'],
  [0x00088008, 'Self-timer 10 s, 3 shots', 'Timer 10 s ×3'], [0x00088009, 'Self-timer 10 s, 5 shots', 'Timer 10 s ×5'],
  [0x00048337, 'Bracket 0.3 EV, 3 shots, continuous', 'BRK C 0.3 ×3'], [0x00048537, 'Bracket 0.3 EV, 5 shots, continuous', 'BRK C 0.3 ×5'],
  [0x00048311, 'Bracket 1.0 EV, 3 shots, continuous', 'BRK C 1.0 ×3'], [0x00058336, 'Bracket 0.3 EV, 3 shots, single', 'BRK S 0.3 ×3'],
  [0x00058320, 'Bracket 2.0 EV, 3 shots, single', 'BRK S 2.0 ×3'], [0x00068028, 'White balance bracket Hi', 'WB BRK Hi'],
  [0x00068018, 'White balance bracket Lo', 'WB BRK Lo'], [0x00078029, 'DRO bracket Hi', 'DRO BRK Hi'], [0x00078019, 'DRO bracket Lo', 'DRO BRK Lo'],
];
const FOCUS: [number, string, string][] = [
  [0x0002, 'Single-shot AF', 'AF-S'], [0x8005, 'Automatic AF', 'AF-A'], [0x8004, 'Continuous AF', 'AF-C'],
  [0x8006, 'Direct Manual Focus', 'DMF'], [0x0001, 'Manual Focus', 'MF'],
];
const WBS: [number, string, string][] = [
  [0x0002, 'Auto', 'Auto'], [0x0004, 'Daylight', 'Daylight'], [0x8011, 'Shade', 'Shade'], [0x8010, 'Cloudy', 'Cloudy'],
  [0x0006, 'Incandescent', 'Incandescent'], [0x8001, 'Fluorescent warm white', 'Fluor. warm'], [0x8002, 'Fluorescent cool white', 'Fluor. cool'],
  [0x8003, 'Fluorescent day white', 'Fluor. day'], [0x8004, 'Fluorescent daylight', 'Fluor. light'], [0x0007, 'Flash', 'Flash'],
  [0x8030, 'Underwater auto', 'Underwater'], [0x8012, 'Colour temp', 'Colour temp'], [0x8020, 'Custom 1', 'Custom 1'],
  [0x8021, 'Custom 2', 'Custom 2'], [0x8022, 'Custom 3', 'Custom 3'],
];
const FORMATS: [number, string, string][] = [[1, 'RAW', 'RAW'], [2, 'RAW+JPEG', 'RAW+JPEG'], [3, 'JPEG', 'JPEG']];
const ISOS = [100, 125, 160, 200, 250, 320, 400, 500, 640, 800, 1000, 1250, 1600, 2000, 2500, 3200, 4000, 5000, 6400, 8000, 10000, 12800, 16000, 20000, 25600, 32000];

function prop(code: number, raw: PtpValue, label: string, short: string, choices: [PtpValue, string, string][] = []): PropertySummary {
  return { code, label, short, raw, writable: true, enabled: 'On', choices: choices.map(([r, l, s]) => ({ raw: r, label: l, short: s })) };
}

function pick(list: [number, string, string][], raw: number, code: number): PropertySummary {
  const [, label, short] = list.find(([m]) => m === raw) ?? [raw, `Unknown (0x${raw.toString(16)})`, '?'];
  return prop(code, u32(raw), label, short, list.map(([m, l, s]) => [u32(m), l, s]));
}

function camera(): CameraState {
  const base = cameraState as unknown as CameraState;
  const iso = num('iso', 800);
  const replaced = new Map<number, PropertySummary>([
    [0x500e, pick(MODES, num('mode', 0x00010002), 0x500e)],
    [0x5013, pick(DRIVES, num('drive', 0x00018015), 0x5013)],
    [0x500a, { ...pick(FOCUS, num('focus', 0x8004), 0x500a), raw: { U16: num('focus', 0x8004) }, choices: FOCUS.map(([m, l, s]) => ({ raw: { U16: m }, label: l, short: s })) }],
    [0x5005, { ...pick(WBS, num('wb', 0x8012), 0x5005), raw: { U16: num('wb', 0x8012) }, choices: WBS.map(([m, l, s]) => ({ raw: { U16: m }, label: l, short: s })) }],
    [0x5007, { code: 0x5007, label: 'f/5.6', short: 'f/5.6', raw: { U16: 560 }, writable: true, enabled: 'DisplayOnly', choices: [] }],
    [0xd20f, { code: 0xd20f, label: `${num('kelvin', 5500)}`, short: `${num('kelvin', 5500)}`, raw: { U16: num('kelvin', 5500) }, writable: true, enabled: 'On', choices: [], range: { min: 2500, max: 9900, step: 100 } }],
    [0xd21e, prop(0xd21e, u32(iso), `ISO ${iso}`, String(iso), [[u32(0xffffff), 'ISO Auto', 'Auto'], ...ISOS.map((i): [PtpValue, string, string] => [u32(i), `ISO ${i}`, String(i)])])],
  ]);
  return {
    ...base,
    recording: q.get('recording') === '1',
    shots_to_computer: q.get('cardonly') === '1' ? false : true,
    properties: base.properties.map((p) =>
      p.code === 0xd253
        ? { ...pick(FORMATS, num('format', 1), 0xd253), raw: { U8: num('format', 1) }, choices: FORMATS.map(([m, l, s]) => ({ raw: { U8: m }, label: l, short: s })) }
        : (replaced.get(p.code) ?? p),
    ),
  } as CameraState;
}

function presets(count: number): Preset[] {
  const names = ['Studio portrait', 'Outdoor', 'Night street', 'Product'];
  return names.slice(0, count).map((name, i) => ({ id: `p${i}`, name, model: 'ZV-E10', slot: i + 1, values: [] }));
}

const settings: AppSettings = {
  ...defaultSettings('/Users/you/Pictures/Rimix'),
  layout: q.get('layout') === 'Ring' ? 'Ring' : 'Dock',
  appearance: q.get('theme') === 'Dark' ? 'Dark' : 'Light',
  presets: presets(num('presets', 4)),
  bluetooth: false,
  saveToComputer: q.get('copy') !== '0',
  openAs: q.get('files') === '1' ? 'Files' : ((q.get('openas') as AppSettings['openAs']) ?? 'Remote'),
  copyFeature: q.get('copying') === '1' || q.get('transfers') === '1' || q.has('copy') || q.get('cardonly') === '1',
  recentCameras: [{ ip: '192.168.122.1:15740', model: 'ZV-E10', lastUsed: 1, ssid: 'DIRECT-YKE1:ZV-E10', sshUser: null }],
};

/** A stand-in live view frame. */
async function frame(): Promise<ArrayBuffer> {
  const c = document.createElement('canvas');
  [c.width, c.height] = [1024, 576];
  const g = c.getContext('2d')!;
  const sky = g.createLinearGradient(0, 0, 0, 576);
  sky.addColorStop(0, '#8fb0c9');
  sky.addColorStop(1, '#d9c9a8');
  g.fillStyle = sky;
  g.fillRect(0, 0, 1024, 576);
  g.fillStyle = '#4d5a48';
  g.beginPath();
  g.moveTo(0, 576); g.lineTo(260, 300); g.lineTo(520, 480); g.lineTo(760, 250); g.lineTo(1024, 576);
  g.fill();
  const blob = await new Promise<Blob>((done) => c.toBlob((b) => done(b!), 'image/jpeg', 0.8));
  return blob.arrayBuffer();
}

const connected = q.get('screen') !== 'connect' && q.get('files') !== '1';
// report=<name> puts a real camera's own report behind the interface, untouched:
// a file of docs/camera-reports, without its .json.
const reports = import.meta.glob('../../../docs/camera-reports/*.json', { import: 'default' });
const reportOf = reports[`../../../docs/camera-reports/${q.get('report')}.json`];
const state = reportOf ? ((await reportOf()) as CameraState) : camera();
let saved = settings;

mockIPC(
  (cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case 'get_settings': return saved;
      case 'save_settings': saved = a.settings as AppSettings; return null;
      case 'dev_flags': return { webcam: false };
      case 'get_state': return connected ? state : null;
      case 'list_cameras': return connected ? [{ id: CAMERA_ID, model: state.model, active: true }] : [];
      case 'live_view_frame': return frame();
      case 'live_view_meta':
        return {
          focus_frames: [{ kind: 2, state: 2, cx: 0.5, cy: 0.52, w: 0.1, h: 0.14 }],
          face_frames: [{ kind: 1, state: 1, cx: 0.3, cy: 0.42, w: 0.12, h: 0.2 }],
          tracking_frames: [{ kind: 1, state: 1, cx: 0.72, cy: 0.5, w: 0.16, h: 0.22 }],
          level_deg: 0.4,
          fps: 24,
        };
      // connecting=joining|finding|connecting holds a connect at that step.
      case 'join_network': return q.get('connecting') === 'joining' ? new Promise(() => {}) : null;
      case 'wait_for_camera':
        return q.get('connecting') === 'finding'
          ? new Promise(() => {})
          : { model: 'ZV-E10', friendly_name: 'ZV-E10', ip: '192.168.122.1', ptp_port: 15740, ptp_version: null, pairing_required: null, ssh_supported: null, source: 'Gateway' };
      case 'connect': return new Promise(() => {});
      case 'card_list':
        return [
          { handle: 259, filename: 'C0007.MP4', kind: 'Video', bytes: 912_000_000, size_known: true, captured: '2026-11-11T15:31:00' },
          { handle: 258, filename: 'DSC00412.ARW', kind: 'Raw', bytes: 24_600_000, size_known: true, captured: '2026-11-11T15:28:10' },
          { handle: 257, filename: 'DSC00412.JPG', kind: 'Still', bytes: 6_100_000, size_known: true, captured: '2026-11-11T15:28:10' },
          { handle: 256, filename: 'DSC00411.ARW', kind: 'Raw', bytes: 24_900_000, size_known: true, captured: '2026-11-11T15:20:42' },
        ];
      // files=1: a camera in Send to Smartphone, with one folder of files.
      case 'files_open': return { model: 'ZV-E10', root: 'PushRoot', address: '192.168.122.1:64321' };
      case 'files_list': {
        const f = (id: string, title: string, bytes: number | null, original = true) => ({
          id, title, folder: false, date: '2026-11-11T15:28:10', thumb: `t/${title}`, preview: `p/${title}`,
          file: `o/${title}`, bytes, resolution: bytes && title.endsWith('.JPG') ? '6000x4000' : null, original,
        });
        return a.folder
          ? [f('a', 'DSC00412.JPG', 6_100_000), f('b', 'DSC00413.ARW', null, false), f('c', 'DSC00414.JPG', 5_800_000), f('d', 'C0007.MP4', 912_000_000), f('e', 'DSC00415.JPG', 6_300_000), f('g', 'DSC00416.JPG', 6_000_000)]
          : [{ id: 'd1', title: '2026-11-11', folder: true, date: null, thumb: null, preview: null, file: null, bytes: null, resolution: null, original: false }];
      }
      case 'files_picture': return frame();
      case 'files_import': return a.id === 'a' ? '/Users/you/Pictures/Rimix/DSC00412.JPG' : new Promise(() => {});
      case 'files_close': return null;
      case 'webcam_set': return { enabled: !!a.on, url: 'http://127.0.0.1:8717/', quality: 'Full', width: 1024, height: 576, fps: 24, viewers: 0 };
      case 'discover': case 'ble_scan': case 'shoot_all': case 'apply_values': return [];
      case 'card_mode': return 'same-session';
      case 'sftp_status': return null;
      case 'webcam_status': return { enabled: q.get('webcam') === '1', url: 'http://127.0.0.1:8717/', quality: 'Full', width: 1024, height: 576, fps: 24, viewers: q.get('webcam') === '1' ? 1 : 0 };
      case 'current_network': case 'pick_folder': return null;
      default: return null;
    }
  },
  { shouldMockEvents: true },
);

await import('./main');

const send = (event: unknown) => emit('camera-event', { camera_id: CAMERA_ID, event });
window.setTimeout(() => {
  // hover=<n> shows the nth choice of what the camera is for as hovered.
  if (q.has('hover')) document.querySelectorAll<HTMLElement>('.purpose button')[num('hover', 0)]?.classList.add('hovered');
  if (!connected) {
    // Start a connect from the remembered camera, to show the cover.
    if (q.get('connecting') || q.get('files') === '1') document.querySelector<HTMLElement>('.recent .row-btn button, .recent button')?.click();
    // files=1 goes on to the files screen: one file imported, one arriving; view=1 opens a picture large.
    if (q.get('files') === '1') {
      window.setTimeout(() => {
        const tiles = [...document.querySelectorAll<HTMLElement>('.files-grid .tile')];
        tiles[0]?.querySelector<HTMLElement>('.act button')?.click();
        tiles[2]?.querySelector<HTMLElement>('.act button')?.click();
        if (q.get('view') === '1') tiles[4]?.querySelector<HTMLElement>('.pic')?.click();
      }, 600);
    }
    return;
  }
  void send({ Frame: null });
  void send('Frame');
  if (q.get('transfers') === '1') {
    void send({ CaptureStarted: { filename: 'DSC00412.ARW' } });
    void send({ CaptureProgress: { filename: 'DSC00412.ARW', received: 9_400_000, total: 24_600_000 } });
    void send({ CaptureStarted: { filename: 'C0007.MP4' } });
    void send({ CaptureProgress: { filename: 'C0007.MP4', received: 120_000_000, total: 912_000_000 } });
    void send({ CaptureSaved: { path: '/Users/you/Pictures/Rimix/DSC00411.JPG', bytes: 6_100_000, filename: 'DSC00411.JPG', camera_filename: 'DSC00411.JPG' } });
  }
  // card=1 opens the card browser, settings=1 the App settings.
  const rowNamed = (text: string) => [...document.querySelectorAll<HTMLElement>('.leftcol button')].find((b) => b.textContent === text);
  if (q.get('card') === '1') rowNamed('Browse card')?.click();
  if (q.get('settings') === '1') document.querySelector<HTMLElement>('.gear')?.click();
  const apply = num('apply', 0);
  if (apply) document.querySelectorAll<HTMLElement>('.presets button[role="radio"]')[apply - 1]?.click();
  const open = q.get('open');
  if (open) {
    const chip = [...document.querySelectorAll<HTMLElement>('.chip')].find((c) => c.querySelector('small')?.textContent === open);
    chip?.click();
    // unfold=<setting name> then unfolds another row of the popover, tab=<title> picks a tab in it.
    const unfold = q.get('unfold');
    if (unfold) {
      const row = [...document.querySelectorAll<HTMLElement>('.pop .r')].find((r) => r.querySelector('.k')?.textContent === unfold);
      row?.querySelector<HTMLElement>('.pickbtn')?.click();
    }
    const tab = q.get('tab');
    if (tab) [...document.querySelectorAll<HTMLElement>('.pop [role="tab"]')].find((t) => t.textContent === tab)?.click();
    // In the ring layout the settings are segments of the ring.
    const seg = [...document.querySelectorAll<SVGElement>('.ring .seg')].find((g) => g.textContent?.includes(open));
    seg?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
  }
}, 300);
