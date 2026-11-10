// Webcam QR scanning: samples frames from a <video> through a <canvas> and
// reports the first decoded text. No UI here.
import jsQR from 'jsqr';

export interface Webcam {
  id: string;
  label: string;
}

const MAX_WIDTH = 640;
const DEFAULT_INTERVAL_MS = 125;

export function webcamSupported(nav: Pick<Navigator, 'mediaDevices'> = navigator): boolean {
  return typeof nav.mediaDevices?.getUserMedia === 'function';
}

export async function listWebcams(nav: Pick<Navigator, 'mediaDevices'> = navigator): Promise<Webcam[]> {
  try {
    if (typeof nav.mediaDevices?.enumerateDevices !== 'function') return [];
    const devices = await nav.mediaDevices.enumerateDevices();
    return devices
      .filter((d) => d.kind === 'videoinput')
      .map((d, i) => ({ id: d.deviceId, label: d.label || `Camera ${i + 1}` }));
  } catch {
    return [];
  }
}

export function decodeFrame(data: Uint8ClampedArray, width: number, height: number): string | null {
  const result = jsQR(data, width, height, { inversionAttempts: 'dontInvert' });
  return result ? result.data : null;
}

export interface ScannerDeps {
  getUserMedia(c: MediaStreamConstraints): Promise<MediaStream>;
  decode: typeof decodeFrame;
  now(): number;
}

export interface Scanner {
  start(video: HTMLVideoElement, canvas: HTMLCanvasElement): Promise<void>;
  stop(): void;
}

export function createScanner(
  opts: { deviceId?: string | null; intervalMs?: number; onResult(text: string): void },
  deps: Partial<ScannerDeps> = {},
): Scanner {
  const getUserMedia =
    deps.getUserMedia ?? ((c: MediaStreamConstraints) => navigator.mediaDevices.getUserMedia(c));
  const decode = deps.decode ?? decodeFrame;
  const intervalMs = opts.intervalMs ?? DEFAULT_INTERVAL_MS;

  let stream: MediaStream | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;

  const clearTimer = () => {
    if (timer !== null) {
      clearInterval(timer);
      timer = null;
    }
  };

  let gen = 0;

  const stop = () => {
    gen++;
    clearTimer();
    stream?.getTracks().forEach((t) => t.stop());
    stream = null;
  };

  const constraints = (deviceId: string | null | undefined): MediaStreamConstraints => ({
    video: {
      deviceId: deviceId ? { exact: deviceId } : undefined,
      facingMode: 'user',
      width: { ideal: 1280 },
    },
  });

  /** The saved webcam, or the default one when the saved one is gone. */
  const openWebcam = async (): Promise<MediaStream> => {
    try {
      return await getUserMedia(constraints(opts.deviceId));
    } catch (e) {
      const name = (e as { name?: unknown } | null)?.name;
      const gone = name === 'OverconstrainedError' || name === 'NotFoundError';
      if (!opts.deviceId || !gone) throw e;
      try {
        return await getUserMedia(constraints(null));
      } catch {
        throw e;
      }
    }
  };

  const start = async (video: HTMLVideoElement, canvas: HTMLCanvasElement) => {
    const ctx = canvas.getContext('2d', { willReadFrequently: true });
    if (!ctx) {
      throw Object.assign(new Error('Canvas 2D is not available'), { name: 'NotSupportedError' });
    }
    stop();
    const mine = gen;
    const s = await openWebcam();
    if (gen !== mine) {
      s.getTracks().forEach((t) => t.stop());
      return;
    }
    stream = s;
    video.srcObject = s;
    try {
      await video.play();
    } catch (e) {
      if (gen === mine) stop();
      else s.getTracks().forEach((t) => t.stop());
      throw e;
    }
    if (gen !== mine) {
      s.getTracks().forEach((t) => t.stop());
      return;
    }

    timer = setInterval(() => {
      const vw = video.videoWidth;
      const vh = video.videoHeight;
      if (!vw || !vh) return;
      const w = Math.min(MAX_WIDTH, vw);
      const h = Math.round((vh * w) / vw);
      canvas.width = w;
      canvas.height = h;
      let text: string | null;
      try {
        ctx.drawImage(video, 0, 0, w, h);
        const frame = ctx.getImageData(0, 0, w, h);
        text = decode(frame.data, frame.width, frame.height);
      } catch {
        return;
      }
      if (text !== null) {
        clearTimer();
        opts.onResult(text);
      }
    }, intervalMs);
  };

  return { start, stop };
}
