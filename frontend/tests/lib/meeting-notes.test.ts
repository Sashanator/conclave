import { describe, expect, test } from 'bun:test';
import {
  applyMarks, cleanNoteText, collectMarks, formatRecordingTime, formatSummaryTranscript,
  NOTE_MAX_CHARS, newNote, sortNotes, toggleMark,
} from '../../src/lib/meeting-notes';
import type { Transcript } from '../../src/types';

const t = (p: Partial<Transcript>): Transcript => ({ id: 'x', text: 'hi', timestamp: '10:00:00', ...p });

describe('meeting-notes', () => {
  test('toggleMark sets, replaces and removes', () => {
    expect(toggleMark(null, 'action')).toBe('action');
    expect(toggleMark('action', 'important')).toBe('important');
    expect(toggleMark('important', 'important')).toBeNull();
  });

  test('formatRecordingTime', () => {
    expect(formatRecordingTime(0)).toBe('00:00');
    expect(formatRecordingTime(122.9)).toBe('02:02');
    expect(formatRecordingTime(null)).toBe('--:--');
    expect(formatRecordingTime(undefined)).toBe('--:--');
  });

  test('summary transcript puts marks inline, words not emoji', () => {
    const text = formatSummaryTranscript([
      t({ audio_start_time: 183, text: 'we move it to Q2', mark: 'important' }),
      t({ audio_start_time: 366, text: 'send the deck', mark: 'action' }),
      t({ audio_start_time: 400, text: 'plain' }),
      t({ timestamp: '10:07:00', text: 'no time', mark: 'followup' }),
    ]);
    expect(text).toBe(
      '[03:03] [MARKED: important] we move it to Q2\n' +
      '[06:06] [MARKED: action] send the deck\n' +
      '[06:40] plain\n' +
      '10:07:00 [MARKED: followup] no time',
    );
  });

  test('sortNotes: by time, untimed last, stable, new array', () => {
    const notes = [
      { id: 'a', at: null, text: 'a' }, { id: 'b', at: 9, text: 'b' },
      { id: 'c', at: null, text: 'c' }, { id: 'd', at: 2, text: 'd' },
    ];
    const sorted = sortNotes(notes);
    expect(sorted.map(n => n.id)).toEqual(['d', 'b', 'a', 'c']);
    expect(sorted).not.toBe(notes);
  });

  test('cleanNoteText trims and limits', () => {
    expect(cleanNoteText('  hi ')).toEqual({ ok: true, text: 'hi' });
    expect(cleanNoteText('   ').ok).toBe(false);
    expect(cleanNoteText('x'.repeat(NOTE_MAX_CHARS)).ok).toBe(true);
    expect(cleanNoteText('x'.repeat(NOTE_MAX_CHARS + 1)).ok).toBe(false);
  });

  test('newNote ids are prefixed and unique', () => {
    const a = newNote('a', 1), b = newNote('b', null);
    expect(a.id.startsWith('note-')).toBe(true);
    expect(a.id).not.toBe(b.id);
    expect(b.at).toBeNull();
  });

  test('collectMarks / applyMarks round-trip by sequence_id', () => {
    const live = [t({ id: '1', sequence_id: 1, mark: 'action' }), t({ id: '2', sequence_id: 2 }), t({ id: '3', mark: 'important' })];
    const marks = collectMarks(live);
    expect(marks).toEqual({ '1': 'action' });
    const reloaded = [t({ id: 'seg_1', sequence_id: 1 }), t({ id: 'seg_2', sequence_id: 2 })];
    expect(applyMarks(reloaded, marks).map(x => x.mark ?? null)).toEqual(['action', null]);
    expect(applyMarks(reloaded, undefined)).toEqual(reloaded);
  });
});
