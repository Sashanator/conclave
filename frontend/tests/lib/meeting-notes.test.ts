import { describe, expect, test } from 'bun:test';
import {
  annotationsLocked, applyMarks, cleanNoteText, collectMarks, formatRecordingTime, formatSummaryTranscript,
  marksToPersist, mergeNotes, NOTE_MAX_CHARS, newNote, sortNotes, toggleMark,
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

  test('summary transcript without a name function is byte-identical to today', () => {
    const lines = [
      t({ audio_start_time: 183, text: 'we move it to Q2', mark: 'important' }),
      t({ timestamp: '10:07:00', text: 'no time', mark: 'followup' }),
    ];
    expect(formatSummaryTranscript(lines, undefined)).toBe(formatSummaryTranscript(lines));
    expect(formatSummaryTranscript(lines)).toBe('[03:03] [MARKED: important] we move it to Q2\n10:07:00 [MARKED: followup] no time');
  });

  test('summary transcript puts the speaker name before the text, after any mark', () => {
    const name = (x: Transcript) => (x.speaker_idx === 1 ? 'Сергей' : undefined);
    const text = formatSummaryTranscript([
      t({ audio_start_time: 5, text: 'text', speaker_idx: 1 }),
      t({ audio_start_time: 5, text: 'text', speaker_idx: 1, mark: 'action' }),
      t({ audio_start_time: 9, text: 'unnamed', speaker_idx: 2 }),
    ], name);
    expect(text).toBe(
      '[00:05] Сергей: text\n' +
      '[00:05] [MARKED: action] Сергей: text\n' +
      '[00:09] unnamed',
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

describe('restore and lock helpers', () => {
  test('mergeNotes keeps notes added during the restore and lets state win for the same id', () => {
    const restored = [{ id: 'a', at: 5, text: 'saved' }, { id: 'b', at: 9, text: 'saved too' }];
    const current = [{ id: 'b', at: 9, text: 'edited' }, { id: 'c', at: null, text: 'added during load' }];
    expect(mergeNotes(current, restored)).toEqual([
      { id: 'a', at: 5, text: 'saved' },
      { id: 'b', at: 9, text: 'edited' },
      { id: 'c', at: null, text: 'added during load' },
    ]);
  });

  test('marksToPersist keeps saved marks for lines not loaded yet, and drops cleared loaded lines', () => {
    const saved = { '1': 'important' as const, '2': 'action' as const, '3': 'followup' as const };
    const loaded = [t({ sequence_id: 1, mark: 'important' }), t({ sequence_id: 2, mark: null })];
    expect(marksToPersist(saved, loaded)).toEqual({ '1': 'important', '3': 'followup' });
  });

  test('annotationsLocked is true unless recording and no stop has begun', () => {
    const idle = { isRecording: true, isStopping: false, isProcessing: false, isSaving: false };
    expect(annotationsLocked(idle)).toBe(false);
    expect(annotationsLocked({ ...idle, isRecording: false })).toBe(true);
    expect(annotationsLocked({ ...idle, isStopping: true })).toBe(true);
    expect(annotationsLocked({ ...idle, isProcessing: true })).toBe(true);
    expect(annotationsLocked({ ...idle, isSaving: true })).toBe(true);
  });
});
