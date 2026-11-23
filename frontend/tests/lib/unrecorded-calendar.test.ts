import { afterEach, describe, expect, test } from 'bun:test';
import { readShowUnrecorded, SHOW_UNRECORDED_KEY, writeShowUnrecorded } from '@/lib/calendar-pref';
import { isUnrecordedCalendarRow } from '@/lib/meeting-views';
import type { MeetingRow } from '@/lib/meetings';

const row = (id: string, o: Partial<MeetingRow> = {}): MeetingRow => ({
  id, title: id, titleSource: 'user', templateId: null, emoji: null, reviewed: false, gist: null,
  startedAt: null, endedAt: null, createdAt: '2026-11-10 09:00:00+00:00', calendarUid: null, calendarStart: null, calendarEnd: null,
  calendarCancelledAt: null, hasTranscript: false, hasSummary: false, prepPoints: 0, hasPrep: false, ...o,
});
const rows = [
  row('future', { calendarUid: 'u1', calendarStart: '2026-11-30T10:00:00Z' }),
  row('past', { calendarUid: 'u2', calendarStart: '2026-11-01T10:00:00Z' }),
  row('recorded', { calendarUid: 'u3', calendarStart: '2026-11-02T10:00:00Z', startedAt: '2026-11-02 10:01:00+00:00', hasTranscript: true }),
  row('adhoc', { startedAt: '2026-11-03 10:00:00+00:00', hasTranscript: true }),
];
// What MeetingsTable does with the switch.
const shown = (on: boolean) => (on ? rows : rows.filter((r) => !isUnrecordedCalendarRow(r))).map((r) => r.id);

describe('show calendar events I did not record', () => {
  afterEach(() => localStorage.removeItem(SHOW_UNRECORDED_KEY));
  test('defaults to on and hides nothing', () => {
    expect(readShowUnrecorded()).toBe(true);
    expect(shown(true)).toEqual(['future', 'past', 'recorded', 'adhoc']);
  });
  test('off hides future and past unrecorded calendar rows only', () => {
    expect(shown(false)).toEqual(['recorded', 'adhoc']);
  });
  test('the choice is stored', () => {
    writeShowUnrecorded(false);
    expect(readShowUnrecorded()).toBe(false);
    writeShowUnrecorded(true);
    expect(readShowUnrecorded()).toBe(true);
  });
});
