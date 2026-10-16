import { describe, expect, test } from 'bun:test';
import { daysToSelect, formatMegabytes, recordingCount, selectToDays, shouldRemind, todayKey, toggleId } from '../../src/lib/old-audio';

describe('old-audio helpers', () => {
  test('select mapping', () => {
    expect(daysToSelect(null)).toBe('never');
    expect(daysToSelect(undefined)).toBe('30');
    expect(daysToSelect(7)).toBe('7');
    expect(selectToDays('never')).toBeNull();
    expect(selectToDays('90')).toBe(90);
  });
  test('recording counts are singular for one', () => {
    expect(recordingCount(1)).toBe('1 recording');
    expect(recordingCount(0)).toBe('0 recordings');
    expect(recordingCount(3)).toBe('3 recordings');
  });
  test('sizes', () => {
    expect(formatMegabytes(0)).toBe('0 MB');
    expect(formatMegabytes(12_400_000)).toBe('12 MB');
    expect(formatMegabytes(1_500_000_000)).toBe('1.5 GB');
  });
  test('selection toggles', () => {
    expect(toggleId([], 'a')).toEqual(['a']);
    expect(toggleId(['a', 'b'], 'a')).toEqual(['b']);
  });
  test('reminder at most once a day and only when there is something', () => {
    expect(shouldRemind(null, '2026-11-20', 3)).toBe(true);
    expect(shouldRemind('2026-11-20', '2026-11-20', 3)).toBe(false);
    expect(shouldRemind('2026-11-19', '2026-11-20', 0)).toBe(false);
    expect(todayKey(new Date(2026, 10, 5))).toBe('2026-11-05');
  });
});
