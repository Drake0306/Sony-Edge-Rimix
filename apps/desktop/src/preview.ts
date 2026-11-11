// A browser preview of the app with a pretend camera behind it, for working
// on the interface without the Tauri shell or a camera:
//
//   pnpm dev, then open http://localhost:1420/preview.html
//
// Query parameters: layout=Dock|Ring, theme=Light|Dark, presets=<count>,
// mode=<raw exposure mode>, drive=<raw drive mode>, apply=<slot to apply>, open=<chip name>, transfers=1 (files arriving), recording=1,
// screen=connect, webcam=1. Never part of the built app.

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
  [0x00048337, 'Bracket 0.3 EV, 3 shots, continuous', 'BRK C 0.3 ×3'], [0x00068028, 'White balance bracket Hi', 'WB BRK Hi'],
];
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
    [0xd21e, prop(0xd21e, u32(iso), `ISO ${iso}`, String(iso), [[u32(0xffffff), 'ISO Auto', 'Auto'], ...ISOS.map((i): [PtpValue, string, string] => [u32(i), `ISO ${i}`, String(i)])])],
  ]);
  return {
    ...base,
    recording: q.get('recording') === '1',
    shots_to_computer: q.get('cardonly') === '1' ? false : true,
    properties: base.properties.map((p) => replaced.get(p.code) ?? p),
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

const connected = q.get('screen') !== 'connect';
const state = camera();
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
      case 'live_view_meta': return { width: 1024, height: 576, focus_frames: [] };
      case 'discover': case 'ble_scan': case 'card_list': case 'shoot_all': case 'apply_values': return [];
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
  if (!connected) return;
  void send({ Frame: null });
  void send('Frame');
  if (q.get('transfers') === '1') {
    void send({ CaptureStarted: { filename: 'DSC00412.ARW' } });
    void send({ CaptureProgress: { filename: 'DSC00412.ARW', received: 9_400_000, total: 24_600_000 } });
    void send({ CaptureStarted: { filename: 'C0007.MP4' } });
    void send({ CaptureProgress: { filename: 'C0007.MP4', received: 120_000_000, total: 912_000_000 } });
    void send({ CaptureSaved: { path: '/Users/you/Pictures/Rimix/DSC00411.JPG', bytes: 6_100_000, filename: 'DSC00411.JPG', camera_filename: 'DSC00411.JPG' } });
  }
  const apply = num('apply', 0);
  if (apply) document.querySelectorAll<HTMLElement>('.presets button[role="radio"]')[apply - 1]?.click();
  const open = q.get('open');
  if (open) {
    const chip = [...document.querySelectorAll<HTMLElement>('.chip')].find((c) => c.querySelector('small')?.textContent === open);
    chip?.click();
    // In the ring layout the settings are segments of the ring.
    const seg = [...document.querySelectorAll<SVGElement>('.ring .seg')].find((g) => g.textContent?.includes(open));
    seg?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
  }
}, 300);
