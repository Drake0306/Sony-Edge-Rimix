// What the UI can ask for. Each action calls a command and reports a
// failure as a toast; state changes arrive later as camera events.

import * as ipc from './ipc';
import {
  askToJoinText,
  connectErrorMessage,
  connectingText,
  findingText,
  joiningText,
  wifiErrorMessage,
} from './copy';
import type { AppSettings, Preset, PtpValue, RecentCamera } from './ipc';
import { describeFailures, upsertPreset } from './presets';
import type { WifiCredentials } from './qr/payload';
import type { AppStore } from './state';

/** How long to wait for the camera after joining its network. */
export const FIND_MS = 30_000;
/** How long to wait while the user joins the network from the Wi-Fi menu. */
export const ASK_FIND_MS = 90_000;

export interface Actions {
  /** Connects by address; `ssid` is remembered with the camera when known. */
  connect(ip: string, ssid?: string | null): Promise<boolean>;
  /** Joins the camera's network (or waits for the user to), finds the camera and connects. */
  connectWithWifi(creds: WifiCredentials): Promise<boolean>;
  /** Reconnects a remembered camera through its network when known, else its address. */
  connectRecent(rc: RecentCamera): Promise<boolean>;
  forgetCamera(rc: RecentCamera): Promise<void>;
  disconnect(): Promise<void>;
  shoot(): void;
  focus(): void;
  toggleRecord(): void;
  stepProperty(code: number, delta: number): void;
  setProperty(code: number, raw: PtpValue): void;
  saveSettings(settings: AppSettings): Promise<void>;
  /** Writes a preset's values to the camera, toasting any it refused. */
  applyPreset(preset: Preset): Promise<void>;
  /** Stores a preset, replacing the one in its slot for its model. */
  savePreset(preset: Preset): Promise<void>;
  deletePreset(id: string): Promise<void>;
  pickFolder(): Promise<string | null>;
  toast(message: string): void;
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function createActions(store: AppStore, toast: (m: string) => void = () => {}): Actions {
  const run = (p: Promise<unknown>) => void p.catch((e) => toast(message(e)));

  async function saveSettings(settings: AppSettings): Promise<void> {
    try {
      await ipc.saveSettings(settings);
      store.set({ settings, unsaved: false });
    } catch (e) {
      toast(message(e));
    }
  }

  /** A connect in flight is never doubled, whichever way it was started. */
  const connectOnce = (ip: string, ssid: string | null, name: string) =>
    store.get().connecting ? Promise.resolve(false) : connectTo(ip, ssid, name);

  /** `name` is what the progress line calls the camera. */
  async function connectTo(ip: string, ssid: string | null, name: string): Promise<boolean> {
    store.set({ error: null, connecting: { phase: 'connecting', text: connectingText(name) } });
    try {
      const camera = await ipc.connect(ip, ssid);
      const settings = await ipc.getSettings().catch(() => store.get().settings);
      store.set({
        screen: 'main',
        camera,
        settings,
        connecting: null,
        error: null,
        status: { fps: camera.live_view_fps, lastShotAt: camera.last_shot_at },
      });
      return true;
    } catch (e) {
      const error = connectErrorMessage(message(e), ip.trim());
      // Connecting drops the previous camera first, so after a failed
      // switch there may be no camera left to show.
      const camera = await ipc.getState().catch(() => null);
      store.set(
        camera
          ? { error, camera, connecting: null }
          : { error, camera: null, connecting: null, screen: 'connect', reconnecting: null },
      );
      return false;
    }
  }

  async function connectWithWifi({ ssid, password }: WifiCredentials): Promise<boolean> {
    if (store.get().connecting) return false;
    const fail = (step: 'join' | 'find' | 'wait', e: unknown) => {
      store.set({ connecting: null, error: wifiErrorMessage(step, ssid, message(e)) });
      return false;
    };
    const ask = store.get().settings?.wifiJoin === 'Ask';
    if (!ask) {
      store.set({ error: null, connecting: { phase: 'joining', text: joiningText(ssid) } });
      try {
        await ipc.joinNetwork(ssid, password || null);
      } catch (e) {
        return fail('join', e);
      }
    }
    const text = ask ? askToJoinText(ssid) : findingText(ssid);
    store.set({ error: null, connecting: { phase: 'finding', text } });
    let camera: ipc.DiscoveredCamera;
    try {
      camera = await ipc.waitForCamera(ask ? ASK_FIND_MS : FIND_MS);
    } catch (e) {
      return fail(ask ? 'wait' : 'find', e);
    }
    return connectTo(camera.ip, ssid, camera.model || ssid);
  }

  return {
    connect: (ip, ssid) => connectOnce(ip, ssid ?? null, ip.trim()),
    connectWithWifi,
    connectRecent: (rc) =>
      rc.ssid ? connectWithWifi({ ssid: rc.ssid, password: '' }) : connectOnce(rc.ip, null, rc.ip),
    async forgetCamera(rc) {
      const settings = store.get().settings;
      if (!settings) return;
      // Cameras are told apart by network (Wi-Fi Direct cameras share one
      // address); one known only by address is matched by address.
      const same = (c: RecentCamera) => (rc.ssid ? c.ssid === rc.ssid : !c.ssid && c.ip === rc.ip);
      const recentCameras = settings.recentCameras.filter((c) => !same(c));
      const lastKept = recentCameras.some((c) => c.ip === settings.lastIp);
      await saveSettings({ ...settings, recentCameras, lastIp: lastKept ? settings.lastIp : null });
    },
    async disconnect() {
      await ipc.disconnect().catch((e) => toast(message(e)));
      store.set({
        screen: 'connect',
        camera: null,
        reconnecting: null,
        captures: [],
        transfers: {},
        activePreset: null,
      });
    },
    shoot: () => run(ipc.shoot()),
    focus: () => run(ipc.focus()),
    toggleRecord: () => run(ipc.toggleRecord()),
    stepProperty: (code, delta) => run(ipc.stepProperty(code, delta)),
    setProperty: (code, raw) => run(ipc.setProperty(code, raw)),
    saveSettings,
    async applyPreset(preset) {
      store.set({ presetBusy: preset.slot });
      try {
        const outcomes = await ipc.applyValues(preset.values);
        const failed = describeFailures(outcomes);
        if (failed) toast(failed);
        else store.set({ activePreset: preset.slot });
      } catch (e) {
        toast(message(e));
      } finally {
        store.set({ presetBusy: null });
      }
    },
    async savePreset(preset) {
      const settings = store.get().settings;
      if (!settings) return;
      await saveSettings({ ...settings, presets: upsertPreset(settings.presets, preset) });
    },
    async deletePreset(id) {
      const settings = store.get().settings;
      if (!settings) return;
      const gone = settings.presets.find((p) => p.id === id);
      await saveSettings({ ...settings, presets: settings.presets.filter((p) => p.id !== id) });
      // A preset saved later into this slot has not been applied.
      if (gone && store.get().activePreset === gone.slot) store.set({ activePreset: null });
    },
    async pickFolder() {
      try {
        return await ipc.pickFolder();
      } catch (e) {
        toast(message(e));
        return null;
      }
    },
    toast,
  };
}
