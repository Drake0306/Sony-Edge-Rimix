import { describe, expect, test } from 'vitest';
import { MIN_EVERY_S, start, statusText, tick } from './interval';

describe('interval', () => {
  test('the first shot comes one period after start, then one per period', () => {
    let run = start({ everyS: 5, count: null }, 1000);
    expect(run).toEqual({ plan: { everyS: 5, count: null }, taken: 0, missed: 0, nextAt: 6000, running: true, cameraId: null });

    let r = tick(run, 5999, false);
    expect(r.shoot).toBe(false);
    expect(r.run).toBe(run);

    r = tick(run, 6000, false);
    expect(r.shoot).toBe(true);
    expect(r.run).toMatchObject({ taken: 1, missed: 0, nextAt: 11000, running: true });
    run = r.run;

    r = tick(run, 11100, false);
    expect(r.shoot).toBe(true);
    expect(r.run).toMatchObject({ taken: 2, nextAt: 16000 });
  });

  test('a busy camera skips the shot and counts it as missed', () => {
    const run = start({ everyS: 2, count: null }, 0);
    const r = tick(run, 2000, true);
    expect(r.shoot).toBe(false);
    expect(r.run).toMatchObject({ taken: 0, missed: 1, nextAt: 4000, running: true });
  });

  test('periods missed while the ticker slept count as missed and shoot once', () => {
    const run = start({ everyS: 2, count: null }, 0);
    const r = tick(run, 6500, false);
    expect(r.shoot).toBe(true);
    expect(r.run).toMatchObject({ taken: 1, missed: 2, nextAt: 8000 });
  });

  test('stops once taken and missed reach the count', () => {
    let run = start({ everyS: 2, count: 2 }, 0);
    run = tick(run, 2000, true).run;
    expect(run.running).toBe(true);
    const r = tick(run, 4000, false);
    expect(r.shoot).toBe(true);
    expect(r.run).toMatchObject({ taken: 1, missed: 1, running: false });
    // A stopped run never shoots again.
    expect(tick(r.run, 10000, false).shoot).toBe(false);
  });

  test('start clamps the period to the minimum', () => {
    expect(MIN_EVERY_S).toBe(2);
    expect(start({ everyS: 1, count: null }, 0)).toMatchObject({ plan: { everyS: 2 }, nextAt: 2000 });
  });

  test('statusText', () => {
    const base = { plan: { everyS: 5, count: 20 }, taken: 3, missed: 0, nextAt: 10000, running: true, cameraId: null };
    expect(statusText(base, 6000)).toBe('Interval 3 of 20, next in 4 s');
    expect(statusText({ ...base, plan: { everyS: 5, count: null } }, 6000)).toBe('Interval 3 taken, next in 4 s');
    expect(statusText({ ...base, plan: { everyS: 5, count: null }, missed: 1 }, 6000)).toBe(
      'Interval 3 taken, next in 4 s, 1 missed',
    );
    // Part of a second still to go rounds up.
    expect(statusText(base, 6500)).toBe('Interval 3 of 20, next in 4 s');
  });
});
