// What the UI can ask for. Each action calls a command and reports a
// failure as a toast; state changes arrive later as camera events.

import * as ipc from './ipc';
import { connectErrorMessage } from './copy';
import type { AppSettings, PtpValue } from './ipc';
import type { AppStore } from './state';

export interface Actions {
  connect(ip: string): Promise<boolean>;
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
  return {
    async connect(ip) {
      store.set({ error: null });
      try {
        const camera = await ipc.connect(ip);
        const settings = await ipc.getSettings().catch(() => store.get().settings);
        store.set({
          screen: 'main',
          camera,
          settings,
          status: { fps: camera.live_view_fps, lastShotAt: camera.last_shot_at },
        });
        return true;
      } catch (e) {
        store.set({ error: connectErrorMessage(message(e), ip.trim()) });
        return false;
      }
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
    async saveSettings(settings) {
      try {
        await ipc.saveSettings(settings);
        store.set({ settings });
      } catch (e) {
        toast(message(e));
      }
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
