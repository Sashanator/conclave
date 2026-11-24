import { describe, expect, test } from 'bun:test';
import { autoAttempted, autoSummaryAllowed, markAutoAttempt } from '../../src/lib/auto-summary';

const now = new Date('2026-11-24T12:00:00Z');
const base = { source: null, autoSummaryOn: true, endedAt: '2026-11-24 10:00:00.123456789+00:00', now };

describe('autoSummaryAllowed', () => {
  test('recording path ignores the end time', () => {
    expect(autoSummaryAllowed({ ...base, source: 'recording', endedAt: null })).toBe(true);
  });
  test('setting off blocks both paths', () => {
    expect(autoSummaryAllowed({ ...base, autoSummaryOn: false })).toBe(false);
    expect(autoSummaryAllowed({ ...base, source: 'recording', autoSummaryOn: false })).toBe(false);
  });
  test('recently ended meeting qualifies', () => {
    expect(autoSummaryAllowed(base)).toBe(true);
  });
  test('old, still running, missing or garbage end time does not', () => {
    expect(autoSummaryAllowed({ ...base, endedAt: '2026-11-23 11:59:00+00:00' })).toBe(false);
    expect(autoSummaryAllowed({ ...base, endedAt: null })).toBe(false);
    expect(autoSummaryAllowed({ ...base, endedAt: undefined })).toBe(false);
    expect(autoSummaryAllowed({ ...base, endedAt: 'nope' })).toBe(false);
    expect(autoSummaryAllowed({ ...base, endedAt: '2026-11-25 10:00:00+00:00' })).toBe(false);
  });
});

describe('attempt memory', () => {
  test('remembers a meeting once marked', () => {
    expect(autoAttempted('m-x')).toBe(false);
    markAutoAttempt('m-x');
    expect(autoAttempted('m-x')).toBe(true);
    expect(autoAttempted('m-y')).toBe(false);
  });
});
