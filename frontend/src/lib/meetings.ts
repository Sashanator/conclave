import { invoke } from '@tauri-apps/api/core';
import { format } from 'date-fns';

// Camel-case shape of the Rust `MeetingRow` returned by `api_list_meeting_rows`.
export type MeetingRow = {
  id: string;
  title: string;
  titleSource: string;
  templateId: string | null;
  emoji: string | null;
  reviewed: boolean;
  gist: string | null;
  startedAt: string | null;
  endedAt: string | null;
  createdAt: string;
  calendarUid: string | null;
  calendarStart: string | null;
  calendarEnd: string | null;
  calendarCancelledAt: string | null;
  hasTranscript: boolean;
  hasSummary: boolean;
  prepPoints: number;
};

export type MeetingState = 'recording' | 'finished' | 'cancelled' | 'not-recorded' | 'new';

export type MeetingView = { id: string; name: string; position: number; builtin: boolean; configJson: string };

const DAY_MS = 24 * 60 * 60 * 1000;

/** DB times look like `2026-11-16 10:00:00.123456789+00:00`; RFC 3339 parses as is. Keeps milliseconds. */
export function parseDbTime(s: string): Date {
  const iso = s.replace(' ', 'T').replace(/\.(\d+)/, (_, f: string) => `.${f.slice(0, 3).padEnd(3, '0')}`);
  return new Date(iso);
}

/** One meeting date: the recorded start, else the calendar start, else the creation time. */
export function meetingDate(row: MeetingRow): Date {
  return parseDbTime(row.startedAt ?? row.calendarStart ?? row.createdAt);
}

export function meetingState(row: MeetingRow, liveId: string | null, now: Date): MeetingState {
  if (liveId === row.id) return 'recording';
  if (row.hasTranscript) return 'finished';
  if (row.calendarCancelledAt) return 'cancelled';
  // Not recorded once its start time plus one day has passed.
  return now.getTime() - meetingDate(row).getTime() >= DAY_MS ? 'not-recorded' : 'new';
}

/** "Mon 16 Nov · 10:00–10:45"; the year is added when it differs from `now`'s year. */
export function formatMeetingDate(row: MeetingRow, now: Date, live = false): string {
  const [startRaw, endRaw] = row.startedAt
    ? [row.startedAt, row.endedAt]
    : row.calendarStart
      ? [row.calendarStart, row.calendarEnd]
      : [row.createdAt, null];
  const start = parseDbTime(startRaw);
  const day = format(start, start.getFullYear() === now.getFullYear() ? 'EEE d MMM' : 'EEE d MMM yyyy');
  let time = format(start, 'HH:mm');
  if (live) time += '–now';
  else if (endRaw) time += `–${format(parseDbTime(endRaw), 'HH:mm')}`;
  return `${day} · ${time}`;
}

/** What "Delete meeting" removes, for the confirm dialog. */
export function deleteMeetingParts(row: MeetingRow): string[] {
  const parts: string[] = [];
  if (row.startedAt || row.hasTranscript || row.hasSummary) parts.push('recording, transcript and summary');
  if (row.prepPoints > 0) parts.push(`prep (${row.prepPoints} point${row.prepPoints === 1 ? '' : 's'})`);
  parts.push('notes');
  if (row.calendarUid) parts.push('brief');
  return parts;
}

export const listMeetingRows = () => invoke<MeetingRow[]>('api_list_meeting_rows');

export const createMeeting = (title?: string) => invoke<string>('api_create_meeting', { title });

export const setReviewed = (meetingId: string, reviewed: boolean) =>
  invoke<void>('api_set_meeting_reviewed', { meetingId, reviewed });

export const setTemplate = (meetingId: string, templateId: string | null) =>
  invoke<void>('api_set_meeting_template', { meetingId, templateId });

// The Rust delete responses keep their snake_case key.
type DeleteResult = { folder_warning?: string | null } | null;
const folderWarningOf = (res: DeleteResult) => ({ folderWarning: res?.folder_warning ?? null });

export async function deleteRecording(meetingId: string): Promise<{ folderWarning: string | null }> {
  return folderWarningOf(await invoke<DeleteResult>('api_delete_recording', { meetingId }));
}

export async function deleteMeeting(id: string): Promise<{ folderWarning: string | null }> {
  return folderWarningOf(await invoke<DeleteResult>('api_delete_meeting', { meetingId: id }));
}

export const listViews = () => invoke<MeetingView[]>('api_list_meeting_views');

export const saveView = (view: MeetingView) => invoke<void>('api_save_meeting_view', { view });

export const deleteView = (id: string) => invoke<void>('api_delete_meeting_view', { id });
