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

/** A failed route never blocks the recording: it starts with no target and the save makes a new meeting (§4). */
export async function routeOrFallback(source: StartSourceArg): Promise<RoutedStart> {
  // A new recording starts: an id left by an earlier one must never reach its save.
  try { sessionStorage.removeItem('last_recording_meeting_id'); } catch { /* storage unavailable */ }
  try {
    return await routeRecordingStart(source);
  } catch (error) {
    toast.warning('Recording as a new meeting', { description: String(error) });
    return { meetingId: null, title: null };
  }
}

/** A start failed after routing: gives the row back. An "already in progress" error means another start owns it. */
export async function releaseFailedStart(routed: RoutedStart | null, error: unknown): Promise<void> {
  if (!routed?.meetingId) return;
  if ((error instanceof Error ? error.message : String(error)).includes('already in progress')) return;
  try {
    await invoke('api_release_routed_start', { meetingId: routed.meetingId });
  } catch (e) {
    console.error('Could not release the routed meeting row:', e);
    toast.error('An empty meeting was left behind', {
      description: `${String(e)} Delete the empty started meeting by hand.`,
    });
  }
}
