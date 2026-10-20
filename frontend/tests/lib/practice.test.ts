import { describe, expect, test } from 'bun:test';
import {
  type ChatModel, MATERIAL_MAX_BYTES, NOTICE_TEXT, VOICES, blockedReason, characterName, estimateCost, fitsLimit, formatClock, materialKind, minutesLabel, phaseText, planText, type PracticeStatus,
} from '../../src/lib/practice';

describe('estimateCost', () => {
  test('estimate_cost_values: a 30-min session in USD, rounded to cents', () => {
    expect(estimateCost('claude-haiku-5-5', 'chat')).toBe(0.04);
    expect(estimateCost('claude-sonnet-5-5', 'chat')).toBe(0.77);
    expect(estimateCost('claude-opus-5-5', 'chat')).toBe(1.54);
    // Plan and report counts the report plus thinking at the chosen effort
    expect(estimateCost('claude-opus-5-5', 'plan_report', 'medium')).toBe(0.54);
    expect(estimateCost('claude-opus-5-5', 'plan_report', 'high')).toBe(0.64);
    expect(estimateCost('claude-opus-5-5', 'plan_report', 'xhigh')).toBe(0.84);
    expect(estimateCost('claude-sonnet-5-5', 'plan_report', 'medium')).toBe(0.27);
    expect(estimateCost('claude-sonnet-5-5', 'plan_report', 'high')).toBe(0.32);
    expect(estimateCost('claude-sonnet-5-5', 'plan_report', 'xhigh')).toBe(0.42);
  });

  test('without an effort, plan and report counts no thinking', () => {
    expect(estimateCost('claude-opus-5-5', 'plan_report')).toBe(0.44);
  });

  test('an unknown model is priced at the highest rate instead of throwing', () => {
    expect(estimateCost('claude-unknown-9' as ChatModel, 'chat')).toBe(1.54);
  });
});

describe('minutesLabel', () => {
  test('under 30 s is less than a minute, else whole minutes', () => {
    expect(minutesLabel(0)).toBe('<1 min');
    expect(minutesLabel(29)).toBe('<1 min');
    expect(minutesLabel(30)).toBe('1 min');
    expect(minutesLabel(1080)).toBe('18 min');
  });
});

describe('formatClock', () => {
  test('minutes and seconds, two digits each, minutes not capped at an hour', () => {
    expect(formatClock(372)).toBe('06:12');
    expect(formatClock(0)).toBe('00:00');
    expect(formatClock(59.9)).toBe('00:59');
    expect(formatClock(3600)).toBe('60:00');
  });
});

describe('materialKind', () => {
  test('PDF, text and Markdown files, ignoring case; anything else is refused', () => {
    expect(materialKind('thesis.pdf')).toBe('pdf');
    expect(materialKind('THESIS.PDF')).toBe('pdf');
    expect(materialKind('notes.txt')).toBe('text');
    expect(materialKind('abstract.md')).toBe('text');
    expect(materialKind('deck.docx')).toBeNull();
    expect(materialKind('noextension')).toBeNull();
  });
});

describe('fitsLimit', () => {
  test('the total per setup may reach 20 MB exactly, not beyond', () => {
    expect(MATERIAL_MAX_BYTES).toBe(20 * 1024 * 1024);
    expect(fitsLimit(0, MATERIAL_MAX_BYTES)).toBe(true);
    expect(fitsLimit(MATERIAL_MAX_BYTES, 0)).toBe(true);
    expect(fitsLimit(MATERIAL_MAX_BYTES - 10, 10)).toBe(true);
    expect(fitsLimit(MATERIAL_MAX_BYTES - 10, 11)).toBe(false);
    expect(fitsLimit(MATERIAL_MAX_BYTES, 1)).toBe(false);
  });
});

describe('phaseText', () => {
  test('the call view copy, with the character name', () => {
    expect(phaseText('Loading', 'Prof. Weber')).toBe('Loading…');
    expect(phaseText('CharacterSpeaking', 'Prof. Weber')).toBe('Prof. Weber is speaking…');
    expect(phaseText('Thinking', 'Prof. Weber')).toBe('Thinking…');
    expect(phaseText('UserTurn', 'Prof. Weber')).toBe('Your turn — tap to talk');
    expect(phaseText('Listening', 'Prof. Weber')).toBe('Listening… tap when done');
    expect(phaseText('Transcribing', 'Prof. Weber')).toBe('Transcribing…');
  });
});

describe('notices and voices', () => {
  test('the notice copy is exact', () => {
    expect(NOTICE_TEXT).toEqual({
      DidNotCatch: "I didn't catch that — tap and try again",
      TenMinutesLeft: '10 minutes left',
      TurnCapped: 'Answer cut at 5 minutes',
      ReplyFailed: "Couldn't reach Claude — tap to talk and try again",
      MeetingStarted: 'A meeting recording started, so the session ended',
    });
  });

  test('the ten voices match the Rust list', () => {
    expect(VOICES.map((v) => v.id)).toEqual(['M1', 'M2', 'M3', 'M4', 'M5', 'F1', 'F2', 'F3', 'F4', 'F5']);
    expect(VOICES[0].label).toBe('Male voice 1');
    expect(VOICES[9].label).toBe('Female voice 5');
  });
});

describe('blockedReason', () => {
  const ready: PracticeStatus = { has_claude_key: true, engine_is_whisper: true, voices_ready: true, blocked: null, running_session: null };

  test('nothing blocks a ready status', () => {
    expect(blockedReason(ready)).toBeNull();
  });

  test('the backend reason comes first, then the missing key, then the engine', () => {
    expect(blockedReason({ ...ready, blocked: 'Practice is unavailable while a meeting is recording, importing or re-transcribing' }))
      .toBe('Practice is unavailable while a meeting is recording, importing or re-transcribing');
    expect(blockedReason({ ...ready, has_claude_key: false, engine_is_whisper: false }))
      .toBe('Add your Claude API key in Settings to use Practice');
    expect(blockedReason({ ...ready, engine_is_whisper: false }))
      .toBe('Practice needs Whisper as the transcription engine (Settings → Transcription)');
  });
});

describe('characterName and planText', () => {
  test('the name is the text before the first comma', () => {
    expect(characterName('Prof. Weber, strict examiner')).toBe('Prof. Weber');
    expect(characterName('Mike')).toBe('Mike');
    expect(planText('Prof. Weber')).toBe("Preparing Prof. Weber's questions…");
  });
});
