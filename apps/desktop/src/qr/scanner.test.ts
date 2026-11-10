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

  function rig() {
    const tracks = [{ stop: vi.fn() }, { stop: vi.fn() }];
    let n = 0;
    const resolvers: Array<() => void> = [];
    const getUserMedia = vi.fn(() => {
      const track = tracks[n++];
      const stream = { getTracks: () => [track] } as unknown as MediaStream;
      return new Promise<MediaStream>((res) => resolvers.push(() => res(stream)));
    });
    const decode = vi.fn(() => null as string | null);
    const onResult = vi.fn();
    const video = document.createElement('video');
    video.play = vi.fn(async () => {});
    Object.defineProperty(video, 'videoWidth', { value: 640 });
    Object.defineProperty(video, 'videoHeight', { value: 360 });
    const canvas = document.createElement('canvas');
    const ctx = {
      drawImage: vi.fn(),
      getImageData: vi.fn(() => ({ data: new Uint8ClampedArray(4), width: 1, height: 1 })),
    };
    canvas.getContext = vi.fn(() => ctx) as unknown as typeof canvas.getContext;
    const scanner = createScanner(
      { intervalMs: 100, onResult },
      { getUserMedia, decode, now: () => 0 },
    );
    return { tracks, resolvers, decode, onResult, video, canvas, scanner };
  }

  it('releases the camera when stop() runs while getUserMedia is pending', async () => {
    vi.useFakeTimers();
    const r = rig();
    const started = r.scanner.start(r.video, r.canvas);
    r.scanner.stop();
    r.resolvers[0]();
    await started;
    await vi.advanceTimersByTimeAsync(500);
    expect(r.tracks[0].stop).toHaveBeenCalled();
    expect(r.decode).not.toHaveBeenCalled();
    expect(r.onResult).not.toHaveBeenCalled();
  });

  it('keeps only the latest start when start() is called twice while pending', async () => {
    vi.useFakeTimers();
    const r = rig();
    const first = r.scanner.start(r.video, r.canvas);
    const second = r.scanner.start(r.video, r.canvas);
    r.resolvers[0]();
    r.resolvers[1]();
    await Promise.all([first, second]);
    expect(r.tracks[0].stop).toHaveBeenCalled();
    expect(r.tracks[1].stop).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(300);
    expect(r.decode).toHaveBeenCalledTimes(3);
  });

  it('stops the tracks and rethrows when play() rejects', async () => {
    const r = rig();
    const err = Object.assign(new Error('blocked'), { name: 'NotAllowedError' });
    r.video.play = vi.fn(async () => {
      throw err;
    });
    const started = r.scanner.start(r.video, r.canvas);
    r.resolvers[0]();
    await expect(started).rejects.toBe(err);
    expect(r.tracks[0].stop).toHaveBeenCalledTimes(1);
  });

  it('skips a frame when decode throws', async () => {
    vi.useFakeTimers();
    const r = rig();
    r.decode.mockImplementationOnce(() => {
      throw new Error('boom');
    });
    const started = r.scanner.start(r.video, r.canvas);
    r.resolvers[0]();
    await started;
    await vi.advanceTimersByTimeAsync(300);
    expect(r.decode).toHaveBeenCalledTimes(3);
    r.scanner.stop();
  });

  function fakeVideoAndCanvas() {
    const video = document.createElement('video');
    video.play = vi.fn(async () => {});
    const canvas = document.createElement('canvas');
    canvas.getContext = vi.fn(() => ({ drawImage: vi.fn(), getImageData: vi.fn() })) as unknown as typeof canvas.getContext;
    return { video, canvas };
  }

  it.each(['OverconstrainedError', 'NotFoundError'])(
    'falls back to the default webcam when the saved one is gone (%s)',
    async (name) => {
      const stream = { getTracks: () => [] } as unknown as MediaStream;
      const getUserMedia = vi
        .fn<(c: MediaStreamConstraints) => Promise<MediaStream>>()
        .mockRejectedValueOnce(Object.assign(new Error('gone'), { name }))
        .mockResolvedValue(stream);
      const { video, canvas } = fakeVideoAndCanvas();
      const scanner = createScanner({ deviceId: 'old-cam', onResult: vi.fn() }, { getUserMedia });
      await scanner.start(video, canvas);
      expect(getUserMedia).toHaveBeenCalledTimes(2);
      expect((getUserMedia.mock.calls[1][0].video as MediaTrackConstraints).deviceId).toBeUndefined();
      expect(video.srcObject).toBe(stream);
      scanner.stop();
    },
  );

  it('rejects with the first error when the default webcam fails too', async () => {
    const first = Object.assign(new Error('gone'), { name: 'NotFoundError' });
    const getUserMedia = vi
      .fn<(c: MediaStreamConstraints) => Promise<MediaStream>>()
      .mockRejectedValueOnce(first)
      .mockRejectedValueOnce(Object.assign(new Error('none'), { name: 'NotFoundError' }));
    const { video, canvas } = fakeVideoAndCanvas();
    const scanner = createScanner({ deviceId: 'old-cam', onResult: vi.fn() }, { getUserMedia });
    await expect(scanner.start(video, canvas)).rejects.toBe(first);
    expect(getUserMedia).toHaveBeenCalledTimes(2);
  });

  it('does not retry a denied webcam or one with no saved id', async () => {
    const denied = Object.assign(new Error('no'), { name: 'NotAllowedError' });
    const getUserMedia = vi.fn<(c: MediaStreamConstraints) => Promise<MediaStream>>().mockRejectedValue(denied);
    const { video, canvas } = fakeVideoAndCanvas();
    await expect(createScanner({ deviceId: 'cam', onResult: vi.fn() }, { getUserMedia }).start(video, canvas)).rejects.toBe(denied);
    expect(getUserMedia).toHaveBeenCalledTimes(1);

    const missing = Object.assign(new Error('none'), { name: 'NotFoundError' });
    getUserMedia.mockReset().mockRejectedValue(missing);
    await expect(createScanner({ deviceId: null, onResult: vi.fn() }, { getUserMedia }).start(video, canvas)).rejects.toBe(missing);
    expect(getUserMedia).toHaveBeenCalledTimes(1);
  });
});
