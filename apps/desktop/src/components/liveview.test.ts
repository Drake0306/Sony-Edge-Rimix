import { describe, expect, test } from 'vitest';
import { pointOnPicture } from './liveview';

describe('tap to focus', () => {
  // A 16:9 picture letterboxed in a 1000 by 800 scene: bars above and below.
  const box = { x: 0, y: 118.75, w: 1000, h: 562.5 };

  test('a click on the picture is a point from 0 to 1 across and down it', () => {
    expect(pointOnPicture(box, 500, 400)).toEqual({ x: 0.5, y: 0.5 });
    expect(pointOnPicture(box, 0, 118.75)).toEqual({ x: 0, y: 0 });
    const corner = pointOnPicture(box, 1000, 681.25)!;
    expect([corner.x, corner.y]).toEqual([1, 1]);
    expect(pointOnPicture(box, 250, 259.375)).toEqual({ x: 0.25, y: 0.25 });
  });

  test('a click on the bars beside the picture is nowhere', () => {
    expect(pointOnPicture(box, 500, 50)).toBeNull();
    expect(pointOnPicture(box, 500, 750)).toBeNull();
    expect(pointOnPicture({ x: 100, y: 0, w: 800, h: 800 }, 50, 400)).toBeNull();
    expect(pointOnPicture({ x: 0, y: 0, w: 0, h: 0 }, 0, 0)).toBeNull();
  });
});
