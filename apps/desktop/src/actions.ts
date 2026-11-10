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
import type { AppSettings, PtpValue, RecentCamera } from './ipc';
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
    const fail = (step: 'join' | 'find', e: unknown) => {
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
      return fail('find', e);
    }
    return connectTo(camera.ip, ssid, camera.model || ssid);
  }

  return {
    connect: (ip, ssid) => connectTo(ip, ssid ?? null, ip.trim()),
    connectWithWifi,
    connectRecent: (rc) =>
      rc.ssid ? connectWithWifi({ ssid: rc.ssid, password: '' }) : connectTo(rc.ip, null, rc.ip),
    async forgetCamera(rc) {
      const settings = store.get().settings;
      if (!settings) return;
      await saveSettings({
        ...settings,
        recentCameras: settings.recentCameras.filter((c) => c.ip !== rc.ip),
        lastIp: settings.lastIp === rc.ip ? null : settings.lastIp,
      });
    },
    async disconnect() {
      await ipc.disconnect().catch((e) => toast(message(e)));
      store.set({ screen: 'connect', camera: null, reconnecting: null, captures: [], transfers: {} });
    },
    shoot: () => run(ipc.shoot()),
    focus: () => run(ipc.focus()),
    toggleRecord: () => run(ipc.toggleRecord()),
    stepProperty: (code, delta) => run(ipc.stepProperty(code, delta)),
    setProperty: (code, raw) => run(ipc.setProperty(code, raw)),
    saveSettings,
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
