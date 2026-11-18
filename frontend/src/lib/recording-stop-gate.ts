// Module scope on purpose: the page and the post-processing provider each run useRecordingStop, and both hear the same stop.
let stopInFlight = false;
let heldStarts = 0;
let startCount = 0;
let waiters: Array<() => void> = [];

/** Every instance calls this on `recording-started`; only whether the count changed matters. */
export function noteRecordingStarted(): void {
  startCount += 1;
}

/** Recordings started so far, app-wide: unlike a hook-local counter it keeps counting after the page unmounts. */
export const startsSeen = (): number => startCount;

let stopCount = 0;
/** Every instance calls this on `recording-stopped`; a newer stop owns the session keys. */
export function noteRecordingStopped(): void {
  stopCount += 1;
}
export const stopsSeen = (): number => stopCount;

/** False while another stop's save is running. Every true must be paired with endStop(). */
export function tryBeginStop(): boolean {
  if (stopInFlight) {
    return false;
  }
  stopInFlight = true;
  heldStarts = startCount;
  return true;
}

/**
 * Like tryBeginStop, but a stop for a recording that started after the running save began waits its turn
 * instead of being dropped. A refusal with no start in between is the same stop heard twice: false.
 */
export async function beginStop(): Promise<boolean> {
  while (!tryBeginStop()) {
    if (startCount === heldStarts) return false;
    await new Promise<void>((resolve) => waiters.push(resolve));
  }
  return true;
}

export function endStop(): void {
  stopInFlight = false;
  const w = waiters;
  waiters = [];
  w.forEach((resolve) => resolve());
}
