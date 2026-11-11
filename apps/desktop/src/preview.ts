// A browser preview of the app with a pretend camera behind it, for working
// on the interface without the Tauri shell or a camera:
//
//   pnpm dev, then open http://localhost:1420/preview.html
//
// Query parameters: layout=Dock|Ring, theme=Light|Dark, presets=<count>,
// mode=<raw exposure mode>, apply=<slot to apply>, open=<chip name>, transfers=1 (files arriving), recording=1,
// screen=connect, webcam=1. Never part of the built app.

import { mockIPC } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import cameraState from './fixtures/camera-state.json';
import type { AppSettings, CameraState, Preset, PropertySummary, PtpValue } from './ipc';
import { defaultSettings } from './state';

const q = new URLSearchParams(location.search);
const CAMERA_ID = '192.168.122.1:15740';
const num = (key: string, fallback: number) => (q.has(key) ? Number(q.get(key)) : fallback);

const u16 = (n: number): PtpValue => ({ U16: n });
const u32 = (n: number): PtpValue => ({ U32: n });

const MODES: [number, string][] = [
  [0x8000, 'Auto'], [2, 'P'], [3, 'A'], [4, 'S'], [1, 'M'],
  [0x8054, 'Movie Auto'], [0x8050, 'Movie P'], [0x8053, 'Movie M'], [0x805d, 'S&Q Auto'], [0x8016, 'Handheld twilight'],
];
const ISOS = [50, 64, 80, 100, 125, 160, 200, 250, 320, 400, 500, 640, 800, 1000, 1250, 1600, 2000, 2500, 3200, 6400, 12800, 25600, 32000];

function prop(code: number, raw: PtpValue, label: string, choices: [PtpValue, string][] = []): PropertySummary {
  return { code, label, short: label.replace(/^ISO /, ''), raw, writable: true, enabled: 'On', choices: choices.map(([r, l]) => ({ raw: r, label: l })) };
}

function camera(): CameraState {
  const base = cameraState as unknown as CameraState;
  const mode = num('mode', 0x8000);
  const iso = num('iso', 800);
  const replaced = new Map<number, PropertySummary>([
    [0x500e, prop(0x500e, u16(mode), MODES.find(([m]) => m === mode)?.[1] ?? `Unknown (0x${mode.toString(16)})`, MODES.map(([m, l]) => [u16(m), l]))],
    [0xd21e, prop(0xd21e, u32(iso), `ISO ${iso}`, [[u32(0xffffff), 'ISO Auto'], ...ISOS.map((i): [PtpValue, string] => [u32(i), `ISO ${i}`])])],
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
  }
}, 300);
