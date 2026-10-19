import { describe, expect, test } from 'bun:test';

import { pillLabel, pillTone, showsControls } from '../../src/lib/dictatePill';
import type { PillState, PillView } from '../../src/lib/dictatePill';

const view = (state: PillState, elapsed_ms = 0): PillView => ({ state, elapsed_ms, level: 0, mode: 'cleaned-up' });

describe('dictate pill copy', () => {
  test('labels', () => {
    expect(pillLabel(view('listening'))).toBe('Listening');
    expect(pillLabel(view('hands-free', 42000))).toBe('0:42');
    expect(pillLabel(view('hands-free', 605000))).toBe('10:05');
    expect(pillLabel(view('loading-model'))).toBe('Loading model…');
    expect(pillLabel(view('transcribing'))).toBe('Transcribing…');
    expect(pillLabel(view('pasted'))).toBe('Pasted');
    expect(pillLabel(view('pasted-as-spoken'))).toBe('Pasted as spoken (cleanup failed)');
    expect(pillLabel(view('copied'))).toBe("Couldn't paste — copied, press Ctrl+V");
    expect(pillLabel(view('nothing-heard'))).toBe("Didn't catch that");
    expect(pillLabel(view('no-microphone'))).toBe('No microphone');
  });

  test('controls only in hands-free', () => {
    const states: PillState[] = [
      'listening', 'hands-free', 'loading-model', 'transcribing', 'pasted',
      'pasted-as-spoken', 'copied', 'nothing-heard', 'no-microphone',
    ];
    expect(states.filter(showsControls)).toEqual(['hands-free']);
  });

  test('tones', () => {
    expect(pillTone('copied')).toBe('warn');
    expect(pillTone('no-microphone')).toBe('warn');
    expect(pillTone('pasted')).toBe('ok');
    expect(pillTone('listening')).toBe('live');
    expect(pillTone('hands-free')).toBe('live');
    expect(pillTone('transcribing')).toBe('busy');
  });
});
