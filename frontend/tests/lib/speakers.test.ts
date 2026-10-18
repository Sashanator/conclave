import { describe, expect, test } from 'bun:test';

import {
  formatTalkTime,
  isJobBusy,
  lastAnalysisText,
  lineSpeakerName,
  speakerChips,
} from '../../src/lib/speakers';
import type { SpeakerView } from '../../src/lib/speakers';

const speakers: SpeakerView[] = [
  { idx: 0, name: 'You', person_id: 'self', is_self: true, seconds: 60 },
  { idx: 1, name: 'Сергей', person_id: 'p1', is_self: false, seconds: 30 },
];

describe('lineSpeakerName', () => {
  test('live labels before analysis', () => {
    expect(lineSpeakerName(0, [], false)).toBe('You');
    expect(lineSpeakerName(null, [], false)).toBe('Others');
  });

  test('after analysis null is Unknown and idx resolves', () => {
    expect(lineSpeakerName(1, speakers, true)).toBe('Сергей');
    expect(lineSpeakerName(null, speakers, true)).toBe('Unknown');
  });

  test('after analysis an idx with no speaker is Unknown', () => {
    expect(lineSpeakerName(7, speakers, true)).toBe('Unknown');
  });
});

describe('isJobBusy', () => {
  test('pending and running are busy; everything else is not', () => {
    expect(isJobBusy({ status: 'pending', reason: null, percent: null })).toBe(true);
    expect(isJobBusy({ status: 'running', reason: null, percent: 40 })).toBe(true);
    expect(isJobBusy({ status: 'done', reason: null, percent: 100 })).toBe(false);
    expect(isJobBusy({ status: 'failed', reason: 'boom', percent: null })).toBe(false);
    expect(isJobBusy({ status: 'skipped', reason: 'feature_off', percent: null })).toBe(false);
    expect(isJobBusy(null)).toBe(false);
  });
});

describe('speakerChips', () => {
  test('two speakers named the same person form one chip', () => {
    const chips = speakerChips([
      { idx: 0, name: 'You', person_id: 'self', is_self: true, seconds: 60 },
      { idx: 1, name: 'Сергей', person_id: 'p1', is_self: false, seconds: 30 },
      { idx: 2, name: 'Сергей', person_id: 'p1', is_self: false, seconds: 12 },
    ]);
    expect(chips).toEqual([
      { key: 'self', name: 'You', idxs: [0], seconds: 60, isSelf: true },
      { key: 'p1', name: 'Сергей', idxs: [1, 2], seconds: 42, isSelf: false },
    ]);
  });

  test('self comes first, then the rest by talk time, and unnamed speakers stay apart', () => {
    const chips = speakerChips([
      { idx: 1, name: 'Speaker 2', person_id: null, is_self: false, seconds: 10 },
      { idx: 2, name: 'Speaker 3', person_id: null, is_self: false, seconds: 90 },
      { idx: 0, name: 'You', person_id: 'self', is_self: true, seconds: 5 },
    ]);
    expect(chips.map((c) => c.name)).toEqual(['You', 'Speaker 3', 'Speaker 2']);
    expect(chips.map((c) => c.idxs)).toEqual([[0], [2], [1]]);
  });
});

describe('formatTalkTime', () => {
  test('seconds, minutes, then hours', () => {
    expect(formatTalkTime(0)).toBe('0s');
    expect(formatTalkTime(45)).toBe('45s');
    expect(formatTalkTime(12 * 60 + 40)).toBe('12m');
    expect(formatTalkTime(60 * 60 + 5 * 60)).toBe('1h 05m');
  });
});

describe('lastAnalysisText', () => {
  test('last analysis text', () => {
    expect(lastAnalysisText({ backend: 'directml', run_seconds: 41, audio_seconds: 3480 }))
      .toBe('Last analysis: DirectML · 41 s for a 58-min meeting');
  });

  test('no analysis yet gives no text', () => {
    expect(lastAnalysisText(null)).toBeNull();
  });
});
