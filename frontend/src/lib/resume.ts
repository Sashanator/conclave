import { toast } from 'sonner';
import type { SaveMeetingResponse } from '@/services/storageService';

/** Where a save landed: the meeting page, and whether the recording joined an earlier meeting. */
export function saveOutcome(r: SaveMeetingResponse): { url: string; continued: boolean; continueError: string | null } {
  const continued = r.continued === true;
  const url = `/meeting-details?id=${r.meeting_id}&source=recording${continued ? '&continued=1' : ''}`;
  return { url, continued, continueError: r.continue_error ?? null };
}

/** An earlier meeting the current call can continue into (Rust `ContinueMeeting`). */
export interface ContinueMeeting {
  meeting_id: string;
  title: string;
}

/** Rust `ContinueStatus`: the offer for this call, and the target the recording will join. */
export interface ContinueStatus {
  offer: ContinueMeeting | null;
  target: ContinueMeeting | null;
}

/** Meeting page's "Resume recording": the next start continues this meeting, then goes home to start it. */
export function requestResume(meetingId: string, push: (url: string) => void): void {
  sessionStorage.setItem('autoStartContinueId', meetingId);
  sessionStorage.setItem('autoStartRecording', 'true');
  push('/');
}

/** Reads and clears the continue id, so a later auto-start does not reuse it. */
export function takeAutoStartContinueId(storage: Storage): string | null {
  const id = storage.getItem('autoStartContinueId');
  storage.removeItem('autoStartContinueId');
  return id;
}

export function continueFailed(error: unknown): void {
  toast.error('Could not continue the earlier meeting', {
    description: `${error} This recording will be saved as a new meeting.`,
  });
}
