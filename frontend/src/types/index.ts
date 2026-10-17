export interface Message {
  id: string;
  content: string;
  timestamp: string;
}

export interface Transcript {
  id: string;
  text: string;
  timestamp: string; // Wall-clock time (e.g., "14:30:05")
  sequence_id?: number;
  chunk_start_time?: number; // Legacy field
  is_partial?: boolean;
  confidence?: number;
  // NEW: Recording-relative timestamps for playback sync
  audio_start_time?: number; // Seconds from recording start (e.g., 125.3)
  audio_end_time?: number;   // Seconds from recording start (e.g., 128.6)
  duration?: number;          // Segment duration in seconds (e.g., 3.3)
  mark?: TranscriptMark | null;
  speaker_idx?: number | null; // 0 = "You" (live label); null until speaker labelling
  sentences?: [number, number, string][] | null; // [t0, t1, text] in meeting seconds
}

export type TranscriptMark = 'important' | 'action' | 'followup';

export interface MeetingNote {
  id: string;
  at: number | null; // seconds into the recording; null = added after the meeting
  text: string;
}

export interface TranscriptUpdate {
  text: string;
  timestamp: string; // Wall-clock time for reference
  source: string;
  sequence_id: number;
  chunk_start_time: number; // Legacy field
  is_partial: boolean;
  confidence: number;
  // NEW: Recording-relative timestamps for playback sync
  audio_start_time: number; // Seconds from recording start
  audio_end_time: number;   // Seconds from recording start
  duration: number;          // Segment duration in seconds
  speaker_idx?: number | null;
  sentences?: [number, number, string][] | null;
}

export interface Block {
  id: string;
  type: string;
  content: string;
  color: string;
}

export interface Section {
  title: string;
  blocks: Block[];
}

export interface Summary {
  [key: string]: Section;
}

export interface ApiResponse {
  message: string;
  num_chunks: number;
  data: any[];
}

export interface SummaryResponse {
  status: string;
  summary: Summary;
  raw_summary?: string;
  usage?: {
    prompt_tokens: number;
    completion_tokens: number;
    total_tokens: number;
  };
}

// BlockNote-specific types
export type SummaryFormat = 'legacy' | 'markdown' | 'blocknote';

export interface BlockNoteBlock {
  id: string;
  type: string;
  props?: Record<string, any>;
  content?: any[];
  children?: BlockNoteBlock[];
}

export interface SummaryDataResponse {
  markdown?: string;
  summary_json?: BlockNoteBlock[];
  reasoning_stripped?: boolean;
  template_id?: string;
  template_name?: string;
  template_source?: 'manual' | 'keyword' | 'auto' | 'fallback';
  summary_language?: string;
  language_fallback?: boolean;
  // Legacy format fields
  MeetingName?: string;
  _section_order?: string[];
  [key: string]: any; // For legacy section data
}

export type MeetingSummary = Summary | SummaryDataResponse;

export type SummaryProcessStatus =
  | 'pending'
  | 'processing'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'error'
  | 'idle';

export interface ProcessTranscriptResponse {
  message: string;
  process_id: string;
}

export interface CancelSummaryResponse {
  cancelled: boolean;
  message: string;
  meeting_id: string;
}

export interface SummaryProcessResponse {
  status: SummaryProcessStatus;
  meetingName: string | null;
  meeting_id: string;
  start: string | null;
  end: string | null;
  data: unknown | null;
  error: string | null;
}

// Pagination types for optimized transcript loading
export interface MeetingMetadata {
  id: string;
  title: string;
  created_at: string;
  updated_at: string;
  folder_path?: string;
  keep_audio?: boolean;
}

export interface PaginatedTranscriptsResponse {
  transcripts: Transcript[];
  total_count: number;
  has_more: boolean;
}

// Transcript segment data for virtualized display
export interface TranscriptSegmentData {
  id: string;
  timestamp: number; // audio_start_time in seconds
  endTime?: number; // audio_end_time in seconds
  text: string;
  confidence?: number;
  mark?: TranscriptMark | null;
}

export type SpeakerJobStatus = 'pending' | 'running' | 'done' | 'failed' | 'skipped';

export interface SpeakerView {
  idx: number;
  name: string;
  person_id: string | null;
  is_self: boolean;
  seconds: number;
}

export interface MeetingSpeakers {
  job: { status: SpeakerJobStatus; reason: string | null; percent: number | null } | null;
  speakers: SpeakerView[];
}

export interface SpeakerSettings {
  enabled: boolean;
  models_ready: boolean;
  downloading: boolean;
  last_analysis: { backend: string; run_seconds: number; audio_seconds: number } | null;
}

export interface Person {
  id: string;
  name: string;
  is_self: boolean;
  meetings: number;
  last_heard: string | null;
}
