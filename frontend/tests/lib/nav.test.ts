import { describe, expect, test } from 'bun:test';
import { NAV_MAIN, isNavActive, recordLabel, whatsNewUnseen } from '../../src/lib/nav';

describe('nav', () => {
  test('main items', () => {
    expect(NAV_MAIN.map((i) => i.label)).toEqual(['Meetings', 'Tasks', 'Digests', 'Dictate', 'Practice', 'Stats']);
  });
  test('isNavActive', () => {
    expect(isNavActive('/meeting-details', NAV_MAIN[0])).toBe(true);
    expect(isNavActive('/', NAV_MAIN[0])).toBe(true);
    expect(isNavActive('/ask', NAV_MAIN[0])).toBe(false);
    expect(isNavActive('/tasks', NAV_MAIN[1])).toBe(true);
    expect(isNavActive('/tasks', NAV_MAIN[0])).toBe(false);
  });
  test('recordLabel', () => {
    expect(recordLabel(false, null)).toBe('Record new meeting');
    expect(recordLabel(true, 192)).toBe('Recording 03:12');
    expect(recordLabel(true, null)).toBe('Recording 00:00');
  });
  test('whatsNewUnseen', () => {
    expect(whatsNewUnseen(null, '1.3.0')).toBe(true);
    expect(whatsNewUnseen('1.3.0', '1.3.0')).toBe(false);
    expect(whatsNewUnseen(null, null)).toBe(false);
  });
});
