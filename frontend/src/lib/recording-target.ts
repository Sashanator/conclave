import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';

export type StartSourceArg = 'sidebar' | { page: string };
export interface RoutedStart { meetingId: string | null; title: string }

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
  try {
    return await routeRecordingStart(source);
  } catch (error) {
    toast.warning('Recording as a new meeting', { description: String(error) });
    return { meetingId: null, title: 'Untitled' };
  }
}
