import type { SaveMeetingResponse } from '@/services/storageService';

/** Where a save landed: the meeting page, and whether the recording joined an earlier meeting. */
export function saveOutcome(r: SaveMeetingResponse): { url: string; continued: boolean; continueError: string | null } {
  const continued = r.continued === true;
  const url = `/meeting-details?id=${r.meeting_id}&source=recording${continued ? '&continued=1' : ''}`;
  return { url, continued, continueError: r.continue_error ?? null };
}
