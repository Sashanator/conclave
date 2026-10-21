import { describe, expect, test } from 'bun:test';
import {
  actionLabel, addAlertWord, answerToNote, buildCopilotRequest, resolveAnswerLanguage,
} from '../../src/lib/copilot';
import { NOTE_MAX_CHARS } from '../../src/lib/meeting-notes';
import type { MeetingNote, Transcript } from '../../src/types';

const t = (p: Partial<Transcript>): Transcript => ({ id: 'x', text: 'hi', timestamp: '10:00:00', ...p });
const line = (end: number, text = 'line', extra: Partial<Transcript> = {}): Transcript =>
  t({ audio_start_time: end - 2, audio_end_time: end, text, ...extra });
const note = (p: Partial<MeetingNote>): MeetingNote => ({ id: 'n', at: null, text: 'n', ...p });

describe('copilot request building', () => {
  test('catch_up_window', () => {
    const { request, range } = buildCopilotRequest('catch_up', [
      line(10, 'a'), line(200, 'b'), line(400, 'c'), line(590, 'd'),
    ], [], 'en');
    expect(request.background).toEqual(['[00:08] a', '[03:18] b']);
    expect(request.focus).toEqual(['[06:38] c', '[09:48] d']);
    expect(range).toEqual([290, 590]);
    expect(request.action).toBe('catch_up');
    expect(request.question).toBeNull();
    expect(request.language).toBe('en');
  });

  test('short_meeting_all_focus', () => {
    const { request, range } = buildCopilotRequest('catch_up', [line(50, 'a'), line(100, 'b')], [], 'en');
    expect(request.background).toEqual([]);
    expect(request.focus).toEqual(['[00:48] a', '[01:38] b']);
    expect(range).toEqual([0, 100]);
  });

  test('explain_last_minute', () => {
    const { request, range } = buildCopilotRequest('explain', [
      line(30, 'old'), line(40, 'edge'), line(41, 'in'), line(100, 'last'),
    ], [], 'de');
    expect(request.background).toEqual(['[00:28] old', '[00:38] edge']);
    expect(request.focus).toEqual(['[00:39] in', '[01:38] last']);
    expect(range).toEqual([40, 100]);
    expect(request.action).toBe('explain');
  });

  test('partials_skipped', () => {
    const { request, range } = buildCopilotRequest('ask', [
      line(5, 'final'), line(9, 'partial', { is_partial: true }),
    ], [], 'en', 'why?');
    expect(request.focus).toEqual(['[00:03] final']);
    expect(range).toBeNull();
    // a partial with the largest end time does not move "now"
    const catchUp = buildCopilotRequest('catch_up', [line(400, 'x'), line(900, 'p', { is_partial: true })], [], 'en');
    expect(catchUp.range).toEqual([100, 400]);
  });

  test('you_label', () => {
    const { request } = buildCopilotRequest('ask', [
      line(12, 'mine', { speaker_idx: 0 }), line(14, 'theirs', { speaker_idx: null }),
    ], [], 'en', 'q');
    expect(request.focus).toEqual(['[00:10] You: mine', '[00:12] theirs']);
  });

  test('ask_puts_all_in_focus', () => {
    const { request, range } = buildCopilotRequest('ask', [line(10, 'a'), line(900, 'b')], [], 'en', 'What is the deadline?');
    expect(request.background).toEqual([]);
    expect(request.focus).toEqual(['[00:08] a', '[14:58] b']);
    expect(range).toBeNull();
    expect(request.question).toBe('What is the deadline?');
  });

  test('what_to_ask_puts_all_in_focus_without_question', () => {
    const { request, range } = buildCopilotRequest('what_to_ask', [line(10, 'a'), line(900, 'b')], [], 'en', 'ignored');
    expect(request.background).toEqual([]);
    expect(request.focus).toHaveLength(2);
    expect(range).toBeNull();
    expect(request.question).toBeNull();
  });

  test('notes_sorted_and_formatted', () => {
    const { request } = buildCopilotRequest('catch_up', [line(10)], [
      note({ id: 'a', at: null, text: 'later one' }),
      note({ id: 'b', at: 75, text: 'timed' }),
      note({ id: 'c', at: 3, text: 'early' }),
    ], 'en');
    expect(request.notes).toEqual(['[00:03] early', '[01:15] timed', '[later] later one']);
  });

  test('language_resolution', () => {
    expect(resolveAnswerLanguage('de', 'en')).toBe('de');
    expect(resolveAnswerLanguage(null, 'en')).toBe('en');
    expect(resolveAnswerLanguage(null, 'klingon')).toBe('ru');
    expect(resolveAnswerLanguage(null, null)).toBe('ru');
  });

  test('action_labels', () => {
    expect(actionLabel('catch_up')).toBe('Catch me up');
    expect(actionLabel('what_to_ask')).toBe('What should I ask?');
    expect(actionLabel('explain')).toBe('Explain the last minute');
    expect(actionLabel('ask', ' Why? ')).toBe('Why?');
  });

  test('answer_to_note_cut', () => {
    const note = answerToNote('Catch me up', 'a'.repeat(3000));
    expect([...note]).toHaveLength(NOTE_MAX_CHARS);
    expect(note.endsWith('…')).toBe(true);
    expect(note.startsWith('Copilot (Catch me up): ')).toBe(true);
  });

  test('answer_to_note_short_and_code_points', () => {
    expect(answerToNote('Ask', 'ok')).toBe('Copilot (Ask): ok');
    const cut = answerToNote('Ask', '😀'.repeat(3000));
    expect([...cut]).toHaveLength(NOTE_MAX_CHARS);
    expect([...cut].at(-1)).toBe('…');
  });

  test('alert_word_rules', () => {
    expect(addAlertWord([], '  Sasha ')).toEqual(['Sasha']);
    expect(addAlertWord([], 'Саша')).toEqual(['Саша']);
    expect(addAlertWord([], 'Саша2')).toEqual(['Саша2']);

    const words = ['Sasha'];
    expect(addAlertWord(words, '')).toBe(words);
    expect(addAlertWord(words, '   ')).toBe(words);
    expect(addAlertWord(words, 'Sasha K')).toBe(words);
    expect(addAlertWord(words, "o'neil")).toBe(words);
    expect(addAlertWord(words, 'sasha')).toBe(words);
    expect(addAlertWord(words, 'SASHA')).toBe(words);

    expect(addAlertWord([], 'a'.repeat(40))).toHaveLength(1);
    const tooLong = [] as string[];
    expect(addAlertWord(tooLong, 'a'.repeat(41))).toBe(tooLong);
    expect(addAlertWord([], '𝐀'.repeat(40))).toHaveLength(1); // 40 code points, 80 UTF-16 units
    expect(addAlertWord([], '𝐀'.repeat(41))).toEqual([]);

    const full = Array.from({ length: 30 }, (_, i) => `w${i}`);
    expect(addAlertWord(full, 'extra')).toBe(full);
  });
});
