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

  return {
    async start(video, canvas) {
      const ctx = canvas.getContext('2d', { willReadFrequently: true });
      if (!ctx) {
        throw Object.assign(new Error('Canvas 2D is not available'), { name: 'NotSupportedError' });
      }
      this.stop();
      const s = await getUserMedia({
        video: {
          deviceId: opts.deviceId ? { exact: opts.deviceId } : undefined,
          facingMode: 'user',
          width: { ideal: 1280 },
        },
      });
      stream = s;
      video.srcObject = s;
      await video.play();

      timer = setInterval(() => {
        const vw = video.videoWidth;
        const vh = video.videoHeight;
        if (!vw || !vh) return;
        const w = Math.min(MAX_WIDTH, vw);
        const h = Math.round((vh * w) / vw);
        canvas.width = w;
        canvas.height = h;
        ctx.drawImage(video, 0, 0, w, h);
        const frame = ctx.getImageData(0, 0, w, h);
        const text = decode(frame.data, frame.width, frame.height);
        if (text !== null) {
          clearTimer();
          opts.onResult(text);
        }
      }, intervalMs);
    },
    stop() {
      clearTimer();
      stream?.getTracks().forEach((t) => t.stop());
      stream = null;
    },
  };
}
