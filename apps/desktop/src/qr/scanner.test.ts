import { afterEach, describe, expect, it, vi } from 'vitest';
import QRCode from 'qrcode';
import { webcamErrorMessage } from '../copy';
import { createScanner, decodeFrame, webcamSupported } from './scanner';

/** Rasterises a QR module matrix into RGBA at `scale` px per module with a 4-module quiet zone. */
function raster(text: string, scale = 4): { data: Uint8ClampedArray; width: number; height: number } {
  const { size, data: modules } = QRCode.create(text).modules;
  const quiet = 4;
  const width = (size + quiet * 2) * scale;
  const data = new Uint8ClampedArray(width * width * 4).fill(255);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      if (!modules[y * size + x]) continue;
      for (let dy = 0; dy < scale; dy++) {
        for (let dx = 0; dx < scale; dx++) {
          const i = (((y + quiet) * scale + dy) * width + (x + quiet) * scale + dx) * 4;
          data[i] = data[i + 1] = data[i + 2] = 0;
        }
      }
    }
  }
  return { data, width, height: width };
}

afterEach(() => vi.useRealTimers());

describe('qr scanner', () => {
  it('decodes a QR code from pixels', () => {
    const text = 'W01:S:DIRECT-abc1:ZV-E10;P:12345678;C:ZV-E10;';
    const img = raster(text);
    expect(decodeFrame(img.data, img.width, img.height)).toBe(text);
  });

  it('returns null for a blank frame', () => {
    const blank = new Uint8ClampedArray(64 * 64 * 4).fill(255);
    expect(decodeFrame(blank, 64, 64)).toBeNull();
  });

  it('knows when the webcam API is missing', () => {
    expect(webcamSupported({ mediaDevices: undefined } as unknown as Navigator)).toBe(false);
    expect(
      webcamSupported({ mediaDevices: { getUserMedia: async () => ({}) } } as unknown as Navigator),
    ).toBe(true);
  });

  it('reports the first decode once and stops sampling', async () => {
    vi.useFakeTimers();
    const track = { stop: vi.fn() };
    const stream = { getTracks: () => [track] } as unknown as MediaStream;
    const getUserMedia = vi.fn(async () => stream);
    const decode = vi.fn<(d: Uint8ClampedArray, w: number, h: number) => string | null>();
    decode.mockReturnValueOnce(null).mockReturnValueOnce(null).mockReturnValue('hello');
    const onResult = vi.fn();

    const video = document.createElement('video');
    video.play = vi.fn(async () => {});
    Object.defineProperty(video, 'videoWidth', { value: 1280 });
    Object.defineProperty(video, 'videoHeight', { value: 720 });
    const canvas = document.createElement('canvas');
    const frame = { data: new Uint8ClampedArray(4), width: 640, height: 360 };
    const ctx = { drawImage: vi.fn(), getImageData: vi.fn(() => frame) };
    canvas.getContext = vi.fn(() => ctx) as unknown as typeof canvas.getContext;

    const scanner = createScanner(
      { deviceId: 'cam-1', intervalMs: 100, onResult },
      { getUserMedia, decode, now: () => 0 },
    );
    await scanner.start(video, canvas);

    expect(getUserMedia).toHaveBeenCalledWith({
      video: { deviceId: { exact: 'cam-1' }, facingMode: 'user', width: { ideal: 1280 } },
    });
    expect(video.srcObject).toBe(stream);
    await vi.advanceTimersByTimeAsync(300);
    expect(onResult).toHaveBeenCalledTimes(1);
    expect(onResult).toHaveBeenCalledWith('hello');
    expect(canvas.width).toBe(640);
    await vi.advanceTimersByTimeAsync(300);
    expect(onResult).toHaveBeenCalledTimes(1);
    expect(decode).toHaveBeenCalledTimes(3);
    expect(track.stop).not.toHaveBeenCalled();
    scanner.stop();
    expect(track.stop).toHaveBeenCalledTimes(1);
  });

  it('words webcam errors for the user', () => {
    expect(webcamErrorMessage(Object.assign(new Error('x'), { name: 'NotAllowedError' }))).toMatch(
      /Privacy & Security/,
    );
    expect(webcamErrorMessage(Object.assign(new Error('x'), { name: 'NotFoundError' }))).toMatch(
      /No webcam found/,
    );
    expect(webcamErrorMessage(new Error('weird'))).toMatch(/isn't available here/);
  });
});
