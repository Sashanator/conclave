import type { MeetingNote, Transcript, TranscriptMark } from '@/types';

export const NOTE_MAX_CHARS = 2000;

export const MARKS: ReadonlyArray<{ value: TranscriptMark; icon: string; label: string }> = [
  { value: 'important', icon: '⭐', label: 'Important' },
  { value: 'action', icon: '✅', label: 'Action item' },
  { value: 'followup', icon: '❓', label: 'Follow-up' },
];

export function toggleMark(current: TranscriptMark | null | undefined, chosen: TranscriptMark): TranscriptMark | null {
  return current === chosen ? null : chosen;
}

export function formatRecordingTime(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds)) return '--:--';
  const total = Math.max(0, Math.floor(seconds));
  return `${String(Math.floor(total / 60)).padStart(2, '0')}:${String(total % 60).padStart(2, '0')}`;
}

/** Transcript text for the summary; marks go inline as words (spec §7). */
export function formatSummaryTranscript(transcripts: Transcript[]): string {
  return transcripts
    .map((t) => {
      const time = t.audio_start_time === undefined ? t.timestamp : `[${formatRecordingTime(t.audio_start_time)}]`;
      const mark = t.mark ? ` [MARKED: ${t.mark}]` : '';
      return `${time}${mark} ${t.text}`;
    })
    .join('\n');
}

export function sortNotes(notes: MeetingNote[]): MeetingNote[] {
  return [...notes].sort((a, b) => {
    if (a.at === null && b.at === null) return 0;
    if (a.at === null) return 1;
    if (b.at === null) return -1;
    return a.at - b.at;
  });
}

export function cleanNoteText(text: string): { ok: true; text: string } | { ok: false; error: string } {
  const trimmed = text.trim();
  if (!trimmed) return { ok: false, error: 'A note cannot be empty' };
  if ([...trimmed].length > NOTE_MAX_CHARS) return { ok: false, error: `A note can be at most ${NOTE_MAX_CHARS} characters` };
  return { ok: true, text: trimmed };
}

export function newNote(text: string, at: number | null): MeetingNote {
  return { id: `note-${crypto.randomUUID()}`, at, text };
}

/** Live marks keyed by sequence_id, the one line key that survives a reload (spec §5.1). */
export function collectMarks(transcripts: Transcript[]): Record<string, TranscriptMark> {
  const marks: Record<string, TranscriptMark> = {};
  for (const t of transcripts) {
    if (t.mark && t.sequence_id !== undefined) marks[String(t.sequence_id)] = t.mark;
  }
  return marks;
}

export function applyMarks(transcripts: Transcript[], marks: Record<string, TranscriptMark> | undefined): Transcript[] {
  if (!marks) return transcripts;
  return transcripts.map((t) => {
    const mark = t.sequence_id === undefined ? undefined : marks[String(t.sequence_id)];
    return mark ? { ...t, mark } : t;
  });
}

/** Restored notes join any added while the restore loaded; a note already in state wins. */
export function mergeNotes(current: MeetingNote[], restored: MeetingNote[]): MeetingNote[] {
  const present = new Set(current.map((n) => n.id));
  return [...restored.filter((n) => !present.has(n.id)), ...current];
}

/**
 * Marks to write to the recovery record. A saved mark for a line not loaded yet is kept, so a write
 * made before the backend history arrives cannot drop it.
 */
export function marksToPersist(saved: Record<string, TranscriptMark>, transcripts: Transcript[]): Record<string, TranscriptMark> {
  const loaded = new Set(transcripts.filter((t) => t.sequence_id !== undefined).map((t) => String(t.sequence_id)));
  const kept: Record<string, TranscriptMark> = {};
  for (const [key, mark] of Object.entries(saved)) {
    if (!loaded.has(key)) kept[key] = mark;
  }
  return { ...kept, ...collectMarks(transcripts) };
}

/** Notes and marks freeze once a stop begins: the save takes a snapshot, so later edits would be lost. */
export function annotationsLocked(s: { isRecording: boolean; isStopping: boolean; isProcessing: boolean; isSaving: boolean }): boolean {
  return !s.isRecording || s.isStopping || s.isProcessing || s.isSaving;
}
