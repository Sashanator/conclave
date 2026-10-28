import type { TranscriptSegmentData } from '@/types';

export function findActiveSegmentId(segments: TranscriptSegmentData[], t: number | null | undefined): string | null {
  if (t === null || t === undefined) return null;
  for (let i = segments.length - 1; i >= 0; i--) {
    const s = segments[i];
    if (s.timestamp > t) continue;
    const end = s.endTime ?? segments[i + 1]?.timestamp ?? Infinity;
    return t < end ? s.id : null;
  }
  return null;
}

/** Dispatched on `window` after the old-audio review deleted audio, so an open player refetches. */
export const AUDIO_DELETED_EVENT = 'conclave:audio-deleted';

export interface MeetingAudioInfo {
  path: string | null;
  keep_audio: boolean;
  audio_deleted_at: string | null;
  /** The audio can take a new part recorded on (Resume recording) */
  resumable: boolean;
}

export function audioMissingMessage(info: MeetingAudioInfo | null): string | null {
  if (!info || info.path) return null;
  if (info.audio_deleted_at) return `Audio deleted on ${new Date(info.audio_deleted_at).toLocaleDateString()}`;
  return 'Audio file not found';
}
