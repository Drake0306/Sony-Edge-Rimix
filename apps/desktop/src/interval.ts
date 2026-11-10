// The interval timer's schedule, kept pure: the app's ticker calls `tick`
// with the time and whether the camera is busy, and shoots when told.

/** The shortest period between shots, in seconds. */
export const MIN_EVERY_S = 2;

export interface IntervalPlan {
  everyS: number;
  /** How many shots to take, or null to run until stopped. */
  count: number | null;
}

export interface IntervalRun {
  plan: IntervalPlan;
  taken: number;
  /** Shots skipped because the camera was busy or the ticker was late. */
  missed: number;
  /** When the next shot is due, in ms since the epoch. */
  nextAt: number;
  running: boolean;
  /** The camera the run shoots, whichever is active later; null for the active one. */
  cameraId: string | null;
}

const clampEvery = (s: number) => Math.max(MIN_EVERY_S, Number.isFinite(s) ? s : MIN_EVERY_S);

/** A run on camera `cameraId` whose first shot comes one period from `now`. */
export function start(plan: IntervalPlan, now: number, cameraId: string | null = null): IntervalRun {
  const everyS = clampEvery(plan.everyS);
  return { plan: { ...plan, everyS }, taken: 0, missed: 0, nextAt: now + everyS * 1000, running: true, cameraId };
}

/**
 * Once `now` reaches the next shot, shoots (or counts a miss while the
 * camera is busy) and moves on one period. Periods that passed entirely
 * before this tick count as missed. Returns `run` itself when nothing is due.
 */
export function tick(run: IntervalRun, now: number, cameraBusy: boolean): { run: IntervalRun; shoot: boolean } {
  if (!run.running || now < run.nextAt) return { run, shoot: false };
  const period = run.plan.everyS * 1000;
  const { count } = run.plan;
  let { taken, missed, nextAt } = run;
  const done = () => count !== null && taken + missed >= count;
  // Every period but the latest has already passed unshot.
  while (nextAt + period <= now && !done()) {
    missed += 1;
    nextAt += period;
  }
  let shoot = false;
  if (!done()) {
    if (cameraBusy) missed += 1;
    else {
      taken += 1;
      shoot = true;
    }
    nextAt += period;
  }
  return { run: { ...run, taken, missed, nextAt, running: !done() }, shoot };
}

/** "Interval 3 of 20, next in 4 s" or "Interval 3 taken, next in 4 s", plus ", 1 missed". */
export function statusText(run: IntervalRun, now: number): string {
  const secs = Math.max(0, Math.ceil((run.nextAt - now) / 1000));
  const { count } = run.plan;
  const done = count === null ? `${run.taken} taken` : `${run.taken + run.missed} of ${count}`;
  const missed = run.missed > 0 ? `, ${run.missed} missed` : '';
  return `Interval ${done}, next in ${secs} s${missed}`;
}
