import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';

export type StartSourceArg = 'sidebar' | { page: string };
/** `title: null` lets Rust name the meeting ("Untitled · Mon 14:02"). */
export interface RoutedStart { meetingId: string | null; title: string | null }

/** Asks Rust which meeting row this start belongs to (creating one for a new meeting). */
export function routeRecordingStart(source: StartSourceArg): Promise<{ meetingId: string; title: string }> {
  return invoke<{ meetingId: string; title: string }>('api_route_recording_start', { source });
}

/** Reads and clears the meeting id the calendar/meeting page left for the next auto-start. */
export function takeAutoStartMeetingId(storage: Storage): string | null {
  const id = storage.getItem('autoStartMeetingId');
  storage.removeItem('autoStartMeetingId');
  return id;
}

/** Queues an auto-start into this meeting for the page that mounts `useRecordingStart`; returns that page's url. */
export function queueAutoStart(storage: Storage, meetingId: string): string {
  storage.setItem('autoStartMeetingId', meetingId);
  storage.setItem('autoStartRecording', 'true');
  return queuedPageUrl(storage, meetingId);
}

/** Marks the queued start for one visit of one meeting page: the url carries `start`, the page key is `id:start`. */
export function queuedPageUrl(storage: Storage, meetingId: string): string {
  const start = Date.now();
  storage.setItem('autoStartKey', `${meetingId}:${start}`);
  return `/meeting-details?id=${encodeURIComponent(meetingId)}&start=${start}`;
}

export const pageKeyFor = (meetingId: string | null, start: string | null): string | undefined =>
  meetingId ? `${meetingId}:${start ?? ''}` : undefined;

/** A queued start is for one page visit and one moment: it is skipped on any other page and dropped once stale. */
export const AUTO_START_MAX_AGE_MS = 10_000;
export function autoStartAllowed(storage: Storage, pageKey: string | undefined, now = Date.now()): boolean {
  const key = storage.getItem('autoStartKey');
  if (!key) return true; // tray, detector and sidebar starts carry no key
  const at = Number(key.slice(key.lastIndexOf(':') + 1));
  if (!Number.isFinite(at) || now - at > AUTO_START_MAX_AGE_MS) {
    for (const k of ['autoStartKey', 'autoStartRecording', 'autoStartContinueId', 'autoStartMeetingId']) storage.removeItem(k);
    return false;
  }
  return key === pageKey;
}

/** A failed route never blocks the recording: it starts with no target and the save makes a new meeting (§4). */
export async function routeOrFallback(source: StartSourceArg): Promise<RoutedStart> {
  try {
    return await routeRecordingStart(source);
  } catch (error) {
    toast.warning('Recording as a new meeting', { description: String(error) });
    return { meetingId: null, title: null };
  }
}

/** A start failed after routing: gives the row back. A start that lost a race calls it too: Rust keeps the winner's row. */
export async function releaseFailedStart(routed: RoutedStart | null, _error: unknown): Promise<void> {
  if (!routed?.meetingId) return;
  try {
    await invoke('api_release_routed_start', { meetingId: routed.meetingId });
  } catch (e) {
    console.error('Could not release the routed meeting row:', e);
    toast.error('An empty meeting was left behind', {
      description: `${String(e)} Delete the empty started meeting by hand.`,
    });
  }
}

/**
 * The meeting page shows the Recording view while this meeting records, and while its stop is saved
 * or has failed: the backend clears the live id when the stop lands, but the lines are not safe until the save returns.
 */
export function showRecordingView(a: { meetingId: string | null; liveMeetingId: string | null; heldId: string | null; stopFlow: boolean }): boolean {
  if (!a.meetingId) return false;
  return a.liveMeetingId === a.meetingId || (a.heldId === a.meetingId && a.stopFlow);
}

/** The meeting whose save is running or failed: the stop listener writes its id and only a successful save clears it, so it survives a remount. */
export function heldMeetingId(stopFlow: boolean): string | null {
  if (!stopFlow || typeof sessionStorage === 'undefined') return null;
  try { return sessionStorage.getItem('last_recording_meeting_id'); } catch { return null; }
}
