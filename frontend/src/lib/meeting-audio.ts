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
