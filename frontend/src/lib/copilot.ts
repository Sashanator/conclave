import type { MeetingNote, Transcript } from '../types';
import { formatRecordingTime, formatSummaryTranscript, NOTE_MAX_CHARS, sortNotes } from './meeting-notes';
import { LANGUAGE_OPTIONS } from './summary-languages';

export type CopilotAction = 'catch_up' | 'what_to_ask' | 'explain' | 'ask';

/** Mirrors the Rust `CopilotRequest` sent to `copilot_ask`. */
export interface CopilotRequest {
  action: CopilotAction;
  question: string | null;
  language: string;
  background: string[];
  focus: string[];
  notes: string[];
  /** The recording's recovery id, so the call's usage attaches to the meeting when it is saved. */
  recording_id: string | null;
}

/** Mirrors the Rust `CopilotSettings` (snake_case fields). */
export interface CopilotSettings {
  provider: string | null;
  model: string | null;
  answer_language: string | null;
  alert_words: string[];
}

export const CATCH_UP_SECS = 300;
export const EXPLAIN_SECS = 60;
export const MAX_ALERT_WORDS = 30;
export const MAX_ALERT_WORD_CHARS = 40;
export const MAX_QUESTION_CHARS = 1000;

const LABELS: Record<Exclude<CopilotAction, 'ask'>, string> = {
  catch_up: 'Catch me up',
  what_to_ask: 'What should I ask?',
  explain: 'Explain the last minute',
};

const endOf = (t: Transcript): number => t.audio_end_time ?? t.audio_start_time ?? 0;
const youName = (t: Transcript): string | undefined => (t.speaker_idx === 0 ? 'You' : undefined);
const formatNote = (n: MeetingNote): string => `${n.at === null ? '[later]' : `[${formatRecordingTime(n.at)}]`} ${n.text}`;

/**
 * Splits the final transcript into background and focus lines around the window.
 * `now` is the latest end time, so a short meeting is all focus with range [0, now].
 * what_to_ask and ask see the whole meeting: no background, no range.
 */
export function buildCopilotRequest(
  action: CopilotAction,
  transcripts: Transcript[],
  notes: MeetingNote[],
  language: string,
  question?: string,
  recordingId: string | null = null,
): { request: CopilotRequest; range: [number, number] | null } {
  const finals = transcripts.filter((t) => !t.is_partial);
  const now = finals.reduce((max, t) => Math.max(max, endOf(t)), 0);
  const window = action === 'catch_up' ? CATCH_UP_SECS : action === 'explain' ? EXPLAIN_SECS : null;
  const focusFrom = window === null ? -Infinity : now - window;
  const background: string[] = [];
  const focus: string[] = [];
  for (const t of finals) {
    (endOf(t) <= focusFrom ? background : focus).push(formatSummaryTranscript([t], youName));
  }
  return {
    request: {
      action,
      question: action === 'ask' ? (question ?? null) : null,
      language,
      background,
      focus,
      notes: sortNotes(notes).map(formatNote),
      recording_id: recordingId,
    },
    range: window === null ? null : [Math.max(0, now - window), now],
  };
}

/** The Copilot answer language: the setting, else the pinned summary language if supported, else Russian. */
export function resolveAnswerLanguage(setting: string | null, pinned: string | null): string {
  if (setting) return setting;
  return pinned && LANGUAGE_OPTIONS.some((o) => o.code === pinned) ? pinned : 'ru';
}

/** The card title. A question is its own title. */
export function actionLabel(action: CopilotAction, question?: string): string {
  return action === 'ask' ? (question?.trim() || 'Ask') : LABELS[action];
}

/** Cuts by code point to NOTE_MAX_CHARS, the same measure `cleanNoteText` uses. */
export function answerToNote(label: string, answer: string): string {
  const chars = [...`Copilot (${label}): ${answer}`];
  return chars.length <= NOTE_MAX_CHARS ? chars.join('') : `${chars.slice(0, NOTE_MAX_CHARS - 1).join('')}…`;
}

/** Mirrors the Rust alert-word rules. A rejected word returns `words` itself. */
export function addAlertWord(words: string[], word: string): string[] {
  const w = word.trim();
  if (!/^[\p{L}\p{N}]+$/u.test(w) || [...w].length > MAX_ALERT_WORD_CHARS) return words;
  if (words.length >= MAX_ALERT_WORDS) return words;
  const lower = w.toLowerCase();
  if (words.some((x) => x.toLowerCase() === lower)) return words;
  return [...words, w];
}
