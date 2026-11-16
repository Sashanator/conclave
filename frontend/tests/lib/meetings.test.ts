import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';

const originals = { core: { ...await import('@tauri-apps/api/core') } };

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
});

const invoke = mock(async (_command: string, _args?: Record<string, unknown>): Promise<unknown> => null);
// Keep the real exports so later test files still see listen, Channel and the rest.
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));

const meetings = await import('../../src/lib/meetings');
type MeetingRow = import('../../src/lib/meetings').MeetingRow;

const BASE: MeetingRow = {
  id: 'm1',
  title: 'Sync',
  titleSource: 'user',
  templateId: null,
  emoji: null,
  reviewed: false,
  gist: null,
  startedAt: null,
  endedAt: null,
  createdAt: '2026-11-01T09:00:00Z',
  calendarUid: null,
  calendarStart: null,
  calendarEnd: null,
  calendarCancelledAt: null,
  hasTranscript: false,
  hasSummary: false,
  prepPoints: 0,
};

const local = (y: number, m: number, d: number, h: number, min: number) => new Date(y, m, d, h, min).toISOString();

beforeEach(() => invoke.mockClear());

describe('parseDbTime', () => {
  test('reads DB times with nanoseconds and an offset, keeping milliseconds', () => {
    expect(meetings.parseDbTime('2026-11-16 10:00:00.123456789+00:00').getTime()).toBe(Date.UTC(2026, 10, 16, 10, 0, 0, 123));
  });

  test('reads RFC 3339 with Z', () => {
    expect(meetings.parseDbTime('2026-11-16T10:00:00Z').getTime()).toBe(Date.UTC(2026, 10, 16, 10));
  });
});

describe('meetingState', () => {
  const now = new Date('2026-11-16T10:00:01Z');

  test('the live target is recording, even with a transcript', () => {
    expect(meetings.meetingState({ ...BASE, hasTranscript: true }, 'm1', now)).toBe('recording');
  });

  test('a transcript makes it finished', () => {
    expect(meetings.meetingState({ ...BASE, hasTranscript: true }, null, now)).toBe('finished');
  });

  test('a calendar cancellation makes it cancelled', () => {
    expect(meetings.meetingState({ ...BASE, calendarCancelledAt: '2026-11-15T08:00:00Z' }, null, now)).toBe('cancelled');
  });

  test('not recorded once start plus one day has passed', () => {
    const row = { ...BASE, createdAt: '2026-11-15T10:00:00Z' };
    expect(meetings.meetingState(row, null, new Date('2026-11-16T10:00:01Z'))).toBe('not-recorded');
    expect(meetings.meetingState(row, null, new Date('2026-11-16T09:59:59Z'))).toBe('new');
  });

  test('a future calendar start is new, even with an old creation time', () => {
    const row = { ...BASE, createdAt: '2026-10-01T10:00:00Z', calendarStart: '2026-11-19T17:00:00Z' };
    expect(meetings.meetingState(row, null, now)).toBe('new');
  });
});

describe('formatMeetingDate', () => {
  test('recorded meeting shows start and end', () => {
    const row = { ...BASE, startedAt: local(2026, 10, 16, 10, 0), endedAt: local(2026, 10, 16, 10, 45) };
    expect(meetings.formatMeetingDate(row, new Date(2026, 10, 16, 12, 0))).toBe('Mon 16 Nov · 10:00–10:45');
  });

  test('live meeting shows now as the end', () => {
    const row = { ...BASE, startedAt: local(2026, 10, 16, 10, 49) };
    expect(meetings.formatMeetingDate(row, new Date(2026, 10, 16, 10, 50), true)).toBe('Mon 16 Nov · 10:49–now');
  });

  test('calendar meeting shows the calendar times', () => {
    const row = { ...BASE, calendarStart: local(2026, 10, 19, 17, 0), calendarEnd: local(2026, 10, 19, 18, 0) };
    expect(meetings.formatMeetingDate(row, new Date(2026, 10, 16, 12, 0))).toBe('Thu 19 Nov · 17:00–18:00');
  });

  test('a created-only meeting shows its start time', () => {
    const row = { ...BASE, createdAt: local(2026, 10, 16, 14, 2) };
    expect(meetings.formatMeetingDate(row, new Date(2026, 10, 16, 15, 0))).toBe('Mon 16 Nov · 14:02');
  });

  test('a date in another year includes the year', () => {
    const row = { ...BASE, startedAt: local(2025, 10, 14, 9, 0), endedAt: local(2025, 10, 14, 9, 30) };
    expect(meetings.formatMeetingDate(row, new Date(2026, 10, 16, 12, 0))).toBe('Fri 14 Nov 2025 · 09:00–09:30');
  });
});

describe('deleteMeetingParts', () => {
  test('a finished meeting with prep and a calendar event', () => {
    const row = { ...BASE, hasTranscript: true, startedAt: local(2026, 10, 16, 10, 0), prepPoints: 2, calendarUid: 'uid-1' };
    expect(meetings.deleteMeetingParts(row)).toEqual(['recording, transcript and summary', 'prep (2 points)', 'notes', 'brief']);
  });

  test('one prep point is singular', () => {
    expect(meetings.deleteMeetingParts({ ...BASE, prepPoints: 1 })).toEqual(['prep (1 point)', 'notes']);
  });

  test('a bare new meeting deletes only notes', () => {
    expect(meetings.deleteMeetingParts(BASE)).toEqual(['notes']);
  });
});

describe('commands', () => {
  test('setReviewed sends the meeting id and flag', async () => {
    await meetings.setReviewed('m1', true);
    expect(invoke).toHaveBeenCalledWith('api_set_meeting_reviewed', { meetingId: 'm1', reviewed: true });
  });

  test('saveView sends the view object', async () => {
    const view = { id: 'v1', name: 'Mine', position: 3, builtin: false, configJson: '{}' };
    await meetings.saveView(view);
    expect(invoke).toHaveBeenCalledWith('api_save_meeting_view', { view: { id: 'v1', name: 'Mine', position: 3, builtin: false, configJson: '{}' } });
  });

  test('deleteMeeting maps folder_warning to folderWarning', async () => {
    invoke.mockImplementationOnce(async () => ({ status: 'success', folder_warning: 'kept' }));
    expect(await meetings.deleteMeeting('m1')).toEqual({ folderWarning: 'kept' });
    expect(invoke).toHaveBeenCalledWith('api_delete_meeting', { meetingId: 'm1' });
  });
});
