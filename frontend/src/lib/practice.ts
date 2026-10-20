// Speaking Practice types as the Rust side sends them (frontend/src-tauri/src/practice), and the
// pure helpers and copy the Practice pages share. Field names stay snake_case, as in Rust.

export type MaterialKind = 'pdf' | 'text';
export type ReportStatus = 'pending' | 'waiting' | 'running' | 'ready' | 'failed';

export interface MaterialMeta {
  id: string;
  setup_id: string;
  kind: MaterialKind;
  name: string;
  bytes: number;
}

export interface SetupSummary {
  id: string;
  name: string;
  character: string;
  situation: string;
  voice: string;
  seeded: boolean;
  has_plan: boolean;
  materials: MaterialMeta[];
  session_count: number;
}

export interface SetupInput {
  id: string | null;
  name: string;
  character: string;
  situation: string;
  voice: string;
}

export interface PracticeStatus {
  has_claude_key: boolean;
  engine_is_whisper: boolean;
  voices_ready: boolean;
  blocked: string | null;
  running_session: string | null;
}

export interface SessionSummary {
  id: string;
  setup_name: string;
  started_at: string;
  duration_secs: number;
  report_status: ReportStatus;
  /** e.g. "4 grammar · 31 fillers" */
  summary: string | null;
}

export interface TurnRow {
  session_id: string;
  idx: number;
  speaker: 'user' | 'character';
  live_text: string;
  verbatim_text: string | null;
  at_secs: number;
  wav_offset: number | null;
  wav_len: number | null;
}

export interface Fluency {
  fillers: Record<string, number>;
  filler_total: number;
  words: number;
  speech_secs: number;
  wpm: number;
  long_pauses: number;
}

export type ItemKind = 'Grammar' | 'Unnatural' | 'Clarity';

export interface ReportItem {
  kind: ItemKind;
  turn: number;
  quote: string;
  better: string;
  note: string;
  echoed: boolean;
}

export interface Report {
  version: number;
  fluency: Fluency;
  items: ReportItem[];
  dropped_items: number;
}

export interface SessionDetail {
  summary: SessionSummary;
  character: string;
  turns: TurnRow[];
  report: Report | null;
  report_error: string | null;
  audio_path: string | null;
}

export type Phase = 'Loading' | 'CharacterSpeaking' | 'Thinking' | 'UserTurn' | 'Listening' | 'Transcribing' | 'Ended';
export type Notice = 'DidNotCatch' | 'TenMinutesLeft' | 'TurnCapped' | 'ReplyFailed' | 'MeetingStarted';

// Events
export interface PracticeStatePayload {
  session_id: string;
  phase: Phase;
  character: string;
  elapsed_secs: number;
  caption: string | null;
  notice: Notice | null;
}

export interface PracticeTurnPayload {
  session_id: string;
  idx: number;
  speaker: 'user' | 'character';
  text: string;
}

export interface PracticePrepPayload {
  stage: 'plan' | 'loading' | 'failed';
  error: string | null;
}

export interface VoicesProgressPayload {
  percent: number;
}

export interface ReportStatusPayload {
  session_id: string;
  status: ReportStatus;
}

export const VOICES: { id: string; label: string }[] = [
  { id: 'M1', label: 'Male voice 1' },
  { id: 'M2', label: 'Male voice 2' },
  { id: 'M3', label: 'Male voice 3' },
  { id: 'M4', label: 'Male voice 4' },
  { id: 'M5', label: 'Male voice 5' },
  { id: 'F1', label: 'Female voice 1' },
  { id: 'F2', label: 'Female voice 2' },
  { id: 'F3', label: 'Female voice 3' },
  { id: 'F4', label: 'Female voice 4' },
  { id: 'F5', label: 'Female voice 5' },
];

// Same as MATERIAL_MAX_BYTES in practice/mod.rs: the material total per setup
export const MATERIAL_MAX_BYTES = 20 * 1024 * 1024;

/** mm:ss with the minutes uncapped, so one hour shows as 60:00. */
export function formatClock(secs: number): string {
  const whole = Math.floor(secs);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(Math.floor(whole / 60))}:${pad(whole % 60)}`;
}

/** The kind a file is imported as, by extension (case-insensitive). Null means the file is refused. */
export function materialKind(fileName: string): MaterialKind | null {
  const ext = fileName.slice(fileName.lastIndexOf('.') + 1).toLowerCase();
  if (ext === 'pdf') return 'pdf';
  if (ext === 'txt' || ext === 'md') return 'text';
  return null;
}

/** Whether adding a file of addBytes keeps the setup's material total within the limit. */
export function fitsLimit(existingBytes: number, addBytes: number): boolean {
  return existingBytes + addBytes <= MATERIAL_MAX_BYTES;
}

export const NOTICE_TEXT: Record<Notice, string> = {
  DidNotCatch: "I didn't catch that — tap and try again",
  TenMinutesLeft: '10 minutes left',
  TurnCapped: 'Answer cut at 5 minutes',
  ReplyFailed: "Couldn't reach Claude — tap to talk and try again",
  MeetingStarted: 'A meeting recording started, so the session ended',
};

/** The line under the character while the call runs. Ended has no copy: the page leaves the call view. */
export function phaseText(phase: Phase, name: string): string {
  switch (phase) {
    case 'Loading': return 'Loading…';
    case 'CharacterSpeaking': return `${name} is speaking…`;
    case 'Thinking': return 'Thinking…';
    case 'UserTurn': return 'Your turn — tap to talk';
    case 'Listening': return 'Listening… tap when done';
    case 'Transcribing': return 'Transcribing…';
    case 'Ended': return '';
  }
}

/** The name in the copy: the character text before its first comma ("Prof. Weber, strict examiner" → "Prof. Weber"). */
export function characterName(character: string): string {
  return character.split(',')[0].trim();
}

/** The overlay line while the question plan is made. */
export function planText(name: string): string {
  return `Preparing ${name}'s questions…`;
}

/** Why Practice cannot start, in the order the page shows it. Null means it can. */
export function blockedReason(s: PracticeStatus): string | null {
  if (s.blocked) return s.blocked;
  if (!s.has_claude_key) return 'Add your Claude API key in Settings to use Practice';
  if (!s.engine_is_whisper) return 'Practice needs Whisper as the transcription engine (Settings → Transcription)';
  return null;
}
