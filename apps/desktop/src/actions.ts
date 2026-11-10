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
import type { AppSettings, Preset, PtpValue, RecentCamera, SshLogin } from './ipc';
import { start, type IntervalPlan } from './interval';
import { describeFailures, upsertPreset } from './presets';
import type { WifiCredentials } from './qr/payload';
import type { AppStore } from './state';

/** How long to wait for the camera after joining its network. */
export const FIND_MS = 30_000;
/** How long to wait while the user joins the network from the Wi-Fi menu. */
export const ASK_FIND_MS = 90_000;

export interface Actions {
  /**
   * Connects by address; `ssid` is remembered with the camera when known.
   * `ssh` logs in to a camera with Access Authentication.
   */
  connect(ip: string, ssid?: string | null, ssh?: SshLogin | null): Promise<boolean>;
  /** Joins the camera's network (or waits for the user to), finds the camera and connects. */
  connectWithWifi(creds: WifiCredentials, ssh?: SshLogin | null): Promise<boolean>;
  /**
   * Reconnects a remembered camera through its network when known, else its
   * address. One with Access Authentication opens the connect screen with
   * its username filled in instead, since its password is never kept.
   */
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
  /** Starts the interval timer; the app's ticker shoots on its schedule. */
  startInterval(plan: IntervalPlan): void;
  stopInterval(): void;
  toast(message: string): void;
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The fingerprints in the `connect` error `HOSTKEY <sha256> <md5>`, or null for any other error. */
export function parseHostKey(err: string): { sha256: string; md5: string } | null {
  const m = /^HOSTKEY (\S+) ?(\S*)$/.exec(err);
  return m ? { sha256: m[1], md5: m[2] } : null;
}

/**
 * The fingerprint confirmed earlier for the camera on this network (or at
 * this address when the network is unknown), told apart as in forgetCamera.
 */
function rememberedFingerprint(settings: AppSettings | null, ip: string, ssid: string | null): string | null {
  const same = (c: RecentCamera) => (ssid ? c.ssid === ssid : !c.ssid && c.ip === ip.trim());
  return settings?.recentCameras.find(same)?.sshFingerprint ?? null;
}

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
  const connectOnce = (ip: string, ssid: string | null, name: string, ssh: SshLogin | null) =>
    store.get().connecting ? Promise.resolve(false) : connectTo(ip, ssid, name, ssh);

  /** `name` is what the progress line calls the camera. */
  async function connectTo(ip: string, ssid: string | null, name: string, ssh: SshLogin | null): Promise<boolean> {
    const login =
      ssh && !ssh.fingerprint
        ? { ...ssh, fingerprint: rememberedFingerprint(store.get().settings, ip, ssid) }
        : ssh;
    store.set({
      error: null,
      pendingFingerprint: null,
      sshPrefill: null,
      connecting: { phase: 'connecting', text: connectingText(name) },
    });
    try {
      const camera = await ipc.connect(ip, ssid, login);
      const settings = await ipc.getSettings().catch(() => store.get().settings);
      store.set({
        screen: 'main',
        camera,
        settings,
        connecting: null,
        error: null,
        // Presets belong to a camera; none is active on the new one yet.
        activePreset: null,
        status: { fps: camera.live_view_fps, lastShotAt: camera.last_shot_at },
      });
      return true;
    } catch (e) {
      const hostKey = login ? parseHostKey(message(e)) : null;
      // Connecting drops the previous camera first, so after a failed
      // switch there may be no camera left to show.
      const camera = await ipc.getState().catch(() => null);
      if (hostKey && login) {
        // Not an error: the user compares the fingerprint with the camera's
        // screen on the connect screen, then connects again with it.
        const retry = () =>
          store.get().connecting
            ? Promise.resolve(false)
            : connectTo(ip, ssid, name, { ...login, fingerprint: hostKey.sha256 });
        store.set({ error: null, camera, connecting: null, screen: 'connect', pendingFingerprint: { ...hostKey, retry } });
        return false;
      }
      const error = connectErrorMessage(message(e), ip.trim());
      store.set(
        camera
          ? { error, camera, connecting: null }
          : { error, camera: null, connecting: null, screen: 'connect', reconnecting: null },
      );
      return false;
    }
  }

  async function connectWithWifi({ ssid, password }: WifiCredentials, ssh: SshLogin | null = null): Promise<boolean> {
    if (store.get().connecting) return false;
    store.set({ pendingFingerprint: null });
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
    return connectTo(`${camera.ip}:${camera.ptp_port}`, ssid, camera.model || ssid, ssh);
  }

  /** Drops the camera and everything shown for it, leaving the connect screen up. */
  async function disconnect(): Promise<void> {
    await ipc.disconnect().catch((e) => toast(message(e)));
    store.set({
      screen: 'connect',
      camera: null,
      reconnecting: null,
      captures: [],
      transfers: {},
      activePreset: null,
      interval: null,
    });
  }

  async function connectRecent(rc: RecentCamera): Promise<boolean> {
    if (rc.sshUser) {
      if (store.get().connecting) return false;
      // Like any camera switch, this replaces the connection: the connect
      // screen cannot show a camera that is still live.
      await disconnect();
      store.set({
        screen: 'connect',
        error: null,
        pendingFingerprint: null,
        sshPrefill: { user: rc.sshUser, ssid: rc.ssid, ip: rc.ip },
      });
      return false;
    }
    return rc.ssid ? connectWithWifi({ ssid: rc.ssid, password: '' }) : connectOnce(rc.ip, null, rc.ip, null);
  }

  return {
    connect: (ip, ssid, ssh) => connectOnce(ip, ssid ?? null, ip.trim(), ssh ?? null),
    connectWithWifi,
    connectRecent,
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
    disconnect,
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
    startInterval: (plan) => store.set({ interval: start(plan, Date.now()) }),
    stopInterval: () => store.set({ interval: null }),
    toast,
  };
}
