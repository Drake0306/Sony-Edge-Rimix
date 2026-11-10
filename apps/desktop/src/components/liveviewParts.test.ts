import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { createFrameSwap } from './liveviewParts';

describe('createFrameSwap', () => {
  let created: string[];
  let revoked: string[];
  const make = () => {
    const url = `blob:frame-${created.length + 1}`;
    created.push(url);
    return url;
  };

  beforeEach(() => {
    created = [];
    revoked = [];
    URL.createObjectURL = vi.fn(make);
    URL.revokeObjectURL = vi.fn((url: string) => void revoked.push(url));
  });
  afterEach(() => vi.restoreAllMocks());

  const fire = (img: HTMLImageElement, type: 'load' | 'error') => img.dispatchEvent(new Event(type));
  const notCurrent = (current: string) => created.filter((u) => u !== current);

  test('a frame that fails to decode is revoked and the shown frame stays', () => {
    const img = document.createElement('img');
    const shown = vi.fn();
    const swap = createFrameSwap(img, shown);
    swap.show(URL.createObjectURL(new Blob()));
    fire(img, 'load');
    swap.show(URL.createObjectURL(new Blob()));
    fire(img, 'error');
    expect(shown).toHaveBeenCalledTimes(1);
    expect(revoked.sort()).toEqual(notCurrent('blob:frame-1').sort());
  });

  test('a frame replaced before it loads is revoked, and the old frame after the new one loads', () => {
    const img = document.createElement('img');
    const swap = createFrameSwap(img, () => {});
    swap.show(URL.createObjectURL(new Blob()));
    fire(img, 'load');
    swap.show(URL.createObjectURL(new Blob())); // superseded
    swap.show(URL.createObjectURL(new Blob()));
    fire(img, 'load');
    expect(img.getAttribute('src')).toBe('blob:frame-3');
    expect(revoked.sort()).toEqual(notCurrent('blob:frame-3').sort());
    swap.dispose();
    expect(revoked.sort()).toEqual([...created].sort());
  });
});

describe('parseHex', () => {
  test('reads long and short hex, black otherwise', async () => {
    const { parseHex } = await import('./liveviewParts');
    expect(parseHex('#ff8000')).toEqual([255, 128, 0]);
    expect(parseHex('#0f8')).toEqual([0, 255, 136]);
    expect(parseHex('nope')).toEqual([0, 0, 0]);
  });
});
