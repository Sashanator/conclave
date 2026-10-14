// Module scope on purpose: the page and the post-processing provider each run useRecordingStop, and both hear the same stop.
let stopInFlight = false;

/** False while another stop's save is running. Every true must be paired with endStop(). */
export function tryBeginStop(): boolean {
  if (stopInFlight) {
    return false;
  }
  stopInFlight = true;
  return true;
}

export function endStop(): void {
  stopInFlight = false;
}
