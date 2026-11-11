// What the UI can ask for. Each action calls a command and reports a
// failure as a toast; state changes arrive later as camera events.

import * as ipc from './ipc';
import {
  askToJoinText,
  CANCELLED,
  bleCameraName,
  bleFailedText,
  bleReadingText,
  connectErrorMessage,
  connectingText,
  findingText,
  isBleUnavailable,
  joiningText,
  wifiErrorMessage,
} from './copy';
import { sameAddress } from './address';
import type { AppSettings, FeedQuality, FeedStatus, Found, OpenAs, Preset, PtpValue, RecentCamera, SshLogin } from './ipc';
import { start, type IntervalPlan } from './interval';
import { describeFailures, upsertPreset } from './presets';
import type { WifiCredentials } from './qr/payload';
import {
  activate,
  cameraCount,
  emptyCard,
  transferCamera,
  without,
  withoutHandle,
  type AppState,
  type AppStore,
  type CardState,
} from './state';

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
   * Reads a camera's Wi-Fi details over Bluetooth, then joins and connects
   * as connectWithWifi does. A failure is kept for the camera's row; an
   * unavailable Bluetooth stops the Bluetooth list.
   */
  joinViaBluetooth(found: Found): Promise<boolean>;
  /**
   * Reconnects a remembered camera through its network when known, else its
   * address. One with Access Authentication opens the connect screen with
   * its username filled in instead, since its password is never kept.
   */
  connectRecent(rc: RecentCamera): Promise<boolean>;
  forgetCamera(rc: RecentCamera): Promise<void>;
  /** Disconnects one camera (the active one by default); the next becomes active. */
  disconnect(cameraId?: string): Promise<void>;
  disconnectAll(): Promise<void>;
  /** Makes another connected camera the one the controls act on. */
  switchCamera(cameraId: string): Promise<void>;
  /** Shows the connect screen while the connected cameras stay connected. */
  addCamera(): void;
  /** Shoots one camera, the active one by default. */
  shoot(cameraId?: string | null): void;
  /** Shoots every connected camera at once, toasting any that failed. */
  shootAll(): Promise<void>;
  focus(): void;
  /** Focuses at a point of the live view, `x` and `y` from 0 to 1 across and down the picture. */
  focusAt(x: number, y: number): void;
  /** Stops the connect in progress; nothing is shown as an error. */
  cancelConnect(): Promise<void>;
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
  /** Lists the active camera's card again (or learns it needs its transfer mode). */
  cardRefresh(): Promise<void>;
  /** Downloads one file from the active camera's card; progress arrives as events. */
  cardDownload(handle: number): Promise<void>;
  /** Starts the interval timer on the active camera; the app's ticker shoots on its schedule. */
  startInterval(plan: IntervalPlan): void;
  stopInterval(): void;
  /** Reads Receive from camera's status into the store. */
  refreshSftp(): Promise<void>;
  /** Turns Receive from camera on or off, then reads its status again. */
  setSftpEnabled(enabled: boolean): Promise<void>;
  /** Makes a new password for the camera to log in with, then reads the status again. */
  regenerateSftpPassword(): Promise<void>;
  /** Reads the webcam feed's status into the store. */
  refreshFeed(): Promise<void>;
  /** Turns webcam mode on or off: the camera's picture goes to other apps instead of the screen. */
  setWebcamMode(on: boolean): Promise<void>;
  setWebcamQuality(quality: FeedQuality): Promise<void>;
  toast(message: string): void;
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The fingerprints in the `connect` error `HOSTKEY <sha256> <md5>`, or null for any other error. */
export function parseHostKey(err: string): { sha256: string; md5: string } | null {
  const m = /^HOSTKEY (\S+) ?(\S*)$/.exec(err);
  return m ? { sha256: m[1], md5: m[2] } : null;
}

/**
 * The stored and current fingerprints in the `connect` error
 * `HOSTKEY_CHANGED <expected> <actual_sha256>`, or null for any other error.
 */
export function parseHostKeyChanged(err: string): { expected: string; actual: string } | null {
  const m = /^HOSTKEY_CHANGED (\S+) (\S+)$/.exec(err);
  return m ? { expected: m[1], actual: m[2] } : null;
}

/**
 * The fingerprint confirmed earlier for the camera on this network (or at
 * this address when the network is unknown), told apart as in forgetCamera.
 */
export function rememberedFingerprint(settings: AppSettings | null, ip: string, ssid: string | null): string | null {
  const same = (c: RecentCamera) => (ssid ? c.ssid === ssid : !c.ssid && sameAddress(c.ip, ip));
  return settings?.recentCameras.find(same)?.sshFingerprint ?? null;
}

/** Whether a camera id is an address (`ip:port`) rather than a Wi-Fi network name. */
export const isAddressId = (id: string) => /^(\[[0-9a-fA-F:.]+\]|[0-9.]+):\d+$/.test(id);

/** What showing no camera at all resets. */
const noCameras = (): Partial<AppState> => ({
  screen: 'connect',
  keepCameras: false,
  camera: null,
  cameras: {},
  activeId: null,
  attempts: {},
  reconnecting: null,
  captures: [],
  transfers: {},
  activePreset: null,
  interval: null,
  card: emptyCard(),
  cardOpen: false,
});

/**
 * The store after camera `id` is gone: the next camera (the one after it,
 * else the one before) becomes active if it was, and its files in flight
 * and an interval run on it go too. With no camera left, the connect screen.
 */
function dropCamera(state: AppState, id: string): Partial<AppState> {
  const ids = Object.keys(state.cameras);
  const at = ids.indexOf(id);
  const rest = ids.filter((k) => k !== id);
  if (rest.length === 0) return noCameras();
  const cameras = without(state.cameras, id);
  const attempts = without(state.attempts, id);
  const transfers = Object.fromEntries(Object.entries(state.transfers).filter(([k]) => transferCamera(k) !== id));
  const runHere = state.interval !== null && (state.interval.cameraId ?? state.activeId) === id;
  const patch: Partial<AppState> = { cameras, attempts, transfers, ...(runHere ? { interval: null } : {}) };
  if (state.activeId !== id && state.activeId !== null) return patch;
  const next = rest[Math.min(at, rest.length - 1)];
  return { ...patch, ...activate({ ...state, cameras, attempts }, next), activePreset: null };
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

  async function refreshSftp(): Promise<void> {
    try {
      store.set({ sftp: (await ipc.sftpStatus()) ?? null });
    } catch (e) {
      toast(message(e));
    }
  }

  /** Runs an SFTP command, then reads the status whatever happened. */
  async function sftpThen(p: Promise<unknown>): Promise<void> {
    await p.catch((e) => toast(message(e)));
    await refreshSftp();
  }

  /** Runs a webcam command and keeps the status it answers with. */
  async function feedThen(p: Promise<FeedStatus>): Promise<void> {
    try {
      store.set({ feed: (await p) ?? null });
    } catch (e) {
      toast(message(e));
    }
  }

  /** A connect in flight is never doubled, whichever way it was started. */
  const connectOnce = (ip: string, ssid: string | null, name: string, ssh: SshLogin | null) =>
    store.get().connecting ? Promise.resolve(false) : connectTo(ip, ssid, name, ssh);

  /**
   * After a failed connect: keeps only the cameras still connected (a
   * failed reconnect drops the camera it was for) and returns what changes.
   */
  async function resync(): Promise<Partial<AppState>> {
    const live = await ipc.listCameras().catch(() => null);
    if (!live) return {};
    let state = store.get();
    let patch: Partial<AppState> = {};
    for (const id of Object.keys(state.cameras)) {
      if (live.some((c) => c.id === id)) continue;
      const drop = dropCamera(state, id);
      patch = { ...patch, ...drop };
      state = { ...state, ...drop };
    }
    const active = live.find((c) => c.active)?.id;
    if (active && active !== state.activeId && active in state.cameras) patch = { ...patch, ...activate(state, active) };
    return patch;
  }

  /**
   * Opens a camera just connected on what it was connected for: its picture
   * sent to other apps for a webcam, the card for its files, and for
   * anything else the picture taken back if a webcam had it.
   */
  function openAsChosen(openAs: OpenAs): void {
    const webcam = openAs === 'Webcam';
    if (webcam !== (store.get().feed?.enabled ?? false)) void feedThen(ipc.webcamSet(webcam));
    if (openAs === 'Files') store.set({ cardOpen: true });
  }

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
      const { id, state: camera } = await ipc.connect(ip, ssid, login);
      const settings = await ipc.getSettings().catch(() => store.get().settings);
      // The same camera connected under another id (its network, then its
      // address) was replaced: keep only the cameras the app still has.
      const live = await ipc.listCameras().catch(() => null);
      const kept = live?.some((c) => c.id === id) ? new Set(live.map((c) => c.id)) : null;
      const s = store.get();
      // This camera, and any it replaced, start afresh: their files in
      // flight, reconnect attempts and interval run go.
      const fresh = (k: string) => k === id || (kept !== null && !kept.has(k));
      const others = Object.fromEntries(Object.entries(s.cameras).filter(([k]) => !fresh(k)));
      // A reconnect moves the camera to the end, as the app's list does.
      const cameras = { ...others, [id]: camera };
      const attempts = Object.fromEntries(Object.entries(s.attempts).filter(([k]) => !fresh(k)));
      const transfers = Object.fromEntries(
        Object.entries(s.transfers).filter(([k]) => {
          const owner = transferCamera(k);
          return owner === null || !fresh(owner);
        }),
      );
      const runOn = s.interval === null ? null : (s.interval.cameraId ?? s.activeId);
      const runHere = s.interval !== null && (runOn === null || fresh(runOn));
      store.set({
        screen: 'main',
        keepCameras: false,
        cameras,
        attempts,
        transfers,
        ...activate({ ...s, cameras, attempts }, id),
        settings,
        connecting: null,
        ...(runHere ? { interval: null } : {}),
        error: null,
        // Presets belong to a camera; none is active on the new one yet.
        activePreset: null,
      });
      openAsChosen(settings?.openAs ?? 'Remote');
      return true;
    } catch (e) {
      const hostKey = login ? parseHostKey(message(e)) : null;
      const changed = login ? parseHostKeyChanged(message(e)) : null;
      // A failed reconnect drops the camera it was for, so the store follows
      // the cameras still connected.
      const resynced = await resync();
      const left = cameraCount({ ...store.get(), ...resynced }) > 0;
      // With no camera left, nothing is shown as connected or reconnecting;
      // the last shots stay in the filmstrip for when one is back.
      const synced: Partial<AppState> = left
        ? resynced
        : { camera: null, cameras: {}, activeId: null, attempts: {}, reconnecting: null, interval: null, transfers: {} };
      if (changed) {
        // Never connect silently: show both fingerprints and offer only Cancel.
        store.set({
          ...synced,
          error: null,
          connecting: null,
          screen: 'connect',
          keepCameras: left,
          pendingFingerprint: { sha256: changed.actual, md5: '', retry: () => Promise.resolve(false), changed },
        });
        return false;
      }
      if (hostKey && login) {
        // Not an error: the user compares the fingerprint with the camera's
        // screen on the connect screen, then connects again with it.
        const retry = () =>
          store.get().connecting
            ? Promise.resolve(false)
            : connectTo(ip, ssid, name, { ...login, fingerprint: hostKey.sha256 });
        store.set({
          ...synced,
          error: null,
          connecting: null,
          screen: 'connect',
          keepCameras: left,
          pendingFingerprint: { ...hostKey, retry },
        });
        return false;
      }
      const error = message(e) === CANCELLED ? null : connectErrorMessage(message(e), ip.trim());
      store.set(left ? { ...synced, error, connecting: null } : { ...synced, error, connecting: null, screen: 'connect', keepCameras: false });
      return false;
    }
  }

  async function connectWithWifi({ ssid, password }: WifiCredentials, ssh: SshLogin | null = null): Promise<boolean> {
    if (store.get().connecting) return false;
    const ask = store.get().settings?.wifiJoin === 'Ask';
    // Busy from here on, so a second click during the disconnects below
    // cannot start a second join.
    store.set({
      error: null,
      pendingFingerprint: null,
      connecting: ask ? { phase: 'finding', text: askToJoinText(ssid) } : { phase: 'joining', text: joiningText(ssid) },
    });
    // A camera reached through another Wi-Fi network rejoins it before each
    // reconnect attempt, racing this join: drop those first. Cameras reached
    // by address (on a shared router) stay.
    const { cameras, camera: current, activeId } = store.get();
    const elsewhere = Object.keys(cameras).filter((id) => id !== ssid && !isAddressId(id));
    for (const id of elsewhere) await disconnect(id);
    if (activeId === null && current) await disconnect();
    const fail = (step: 'join' | 'find' | 'wait', e: unknown) => {
      // Cancelled by the user: back to the connect screen, with no complaint.
      const error = message(e) === CANCELLED ? null : wifiErrorMessage(step, ssid, message(e));
      store.set({ connecting: null, error });
      return false;
    };
    if (!ask) {
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

  async function joinViaBluetooth(found: Found): Promise<boolean> {
    if (store.get().connecting) return false;
    store.set({ error: null, connecting: { phase: 'joining', text: bleReadingText(bleCameraName(found)) } });
    let creds: ipc.BleCredentials;
    try {
      creds = await ipc.bleHandoff(found.address);
    } catch (e) {
      const err = message(e);
      if (isBleUnavailable(err)) {
        store.set({ connecting: null, bleUnavailable: true, bleNearby: [], bleFailed: {} });
      } else {
        store.set({ connecting: null, bleFailed: { ...store.get().bleFailed, [found.address]: bleFailedText(err) } });
      }
      return false;
    }
    // The password goes straight into the join and is never kept.
    store.set({ connecting: null, bleFailed: without(store.get().bleFailed, found.address) });
    return connectWithWifi({ ssid: creds.ssid, password: creds.password });
  }

  /**
   * Drops one camera (the active one by default). The next becomes active;
   * after the last, everything shown for it goes and the connect screen is up.
   */
  async function disconnect(cameraId?: string): Promise<void> {
    const id = cameraId ?? store.get().activeId;
    await ipc.disconnect(id).catch((e) => toast(message(e)));
    const state = store.get();
    if (id === null || !(id in state.cameras)) {
      store.set(noCameras());
      return;
    }
    const patch = dropCamera(state, id);
    store.set(patch);
    // The app picks the next camera the same way; say so in case they differ.
    const next = store.get().activeId;
    if (next !== null && next !== state.activeId) await ipc.setActiveCamera(next).catch((e) => toast(message(e)));
  }

  async function disconnectAll(): Promise<void> {
    await ipc.disconnectAll().catch((e) => toast(message(e)));
    store.set(noCameras());
  }

  async function switchCamera(id: string): Promise<void> {
    const state = store.get();
    if (id === state.activeId || !(id in state.cameras)) return;
    try {
      await ipc.setActiveCamera(id);
    } catch (e) {
      toast(message(e));
      return;
    }
    // Presets belong to a camera; none is known to be active on this one.
    store.set({ ...activate(store.get(), id), activePreset: null });
  }

  async function shootAll(): Promise<void> {
    try {
      const results = await ipc.shootAll();
      const { cameras } = store.get();
      for (const [id, error] of results) if (error) toast(`${cameras[id]?.model ?? id}: ${error}`);
    } catch (e) {
      toast(message(e));
    }
  }

  async function connectRecent(rc: RecentCamera): Promise<boolean> {
    if (rc.sshUser) {
      if (store.get().connecting) return false;
      // The connect screen asks for its password; the cameras connected stay.
      store.set({
        screen: 'connect',
        keepCameras: cameraCount(store.get()) > 0,
        error: null,
        pendingFingerprint: null,
        sshPrefill: { user: rc.sshUser, ssid: rc.ssid, ip: rc.ip },
      });
      return false;
    }
    return rc.ssid ? connectWithWifi({ ssid: rc.ssid, password: '' }) : connectOnce(rc.ip, null, rc.ip, null);
  }

  /**
   * Changes the card of camera `id` if it is still the active one: a
   * listing or download that finishes after a switch belongs to a card no
   * longer shown.
   */
  const updateCard = (id: string | null, change: (card: CardState) => Partial<CardState>) => {
    const s = store.get();
    if (s.activeId !== id) return;
    store.set({ card: { ...s.card, ...change(s.card) } });
  };

  async function cardRefresh(): Promise<void> {
    const id = store.get().activeId;
    updateCard(id, () => ({ loading: true, error: null }));
    try {
      const mode = await ipc.cardMode(id);
      const items = mode === 'needs-transfer-mode' ? [] : await ipc.cardList(id);
      updateCard(id, () => ({ items, mode, loading: false }));
    } catch (e) {
      updateCard(id, () => ({ loading: false, error: message(e) }));
    }
  }

  async function cardDownload(handle: number): Promise<void> {
    const { activeId: id, card } = store.get();
    if (handle in card.downloads) return;
    const total = card.items.find((o) => o.handle === handle)?.bytes ?? 0;
    updateCard(id, (c) => ({
      downloads: { ...c.downloads, [handle]: { received: 0, total } },
      failed: withoutHandle(c.failed, handle),
    }));
    // Only the reply settles the row; later progress events are ignored.
    try {
      const path = await ipc.cardDownload(handle, id);
      updateCard(id, (c) => ({ downloads: withoutHandle(c.downloads, handle), done: { ...c.done, [handle]: path } }));
    } catch (e) {
      // Shown on the file's row rather than as a toast.
      const error = message(e);
      updateCard(id, (c) => ({ downloads: withoutHandle(c.downloads, handle), failed: { ...c.failed, [handle]: error } }));
    }
  }

  return {
    connect: (ip, ssid, ssh) => connectOnce(ip, ssid ?? null, ip.trim(), ssh ?? null),
    connectWithWifi,
    joinViaBluetooth,
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
    disconnectAll,
    switchCamera,
    addCamera: () =>
      store.set({ screen: 'connect', keepCameras: true, error: null, pendingFingerprint: null, sshPrefill: null }),
    shoot: (cameraId) => run(ipc.shoot(cameraId ?? null)),
    shootAll,
    focus: () => run(ipc.focus()),
    focusAt: (x, y) => run(ipc.focusAt(x, y)),
    cancelConnect: () => ipc.cancelConnect().catch((e) => toast(message(e))),
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
    startInterval: (plan) => store.set({ interval: start(plan, Date.now(), store.get().activeId) }),
    stopInterval: () => store.set({ interval: null }),
    cardRefresh,
    cardDownload,
    refreshSftp,
    setSftpEnabled: (enabled) => sftpThen(ipc.sftpSetEnabled(enabled)),
    regenerateSftpPassword: () => sftpThen(ipc.sftpRegeneratePassword()),
    refreshFeed: () => feedThen(ipc.webcamStatus()),
    setWebcamMode: (on) => feedThen(ipc.webcamSet(on)),
    setWebcamQuality: (quality) => feedThen(ipc.webcamSetQuality(quality)),
    toast,
  };
}
