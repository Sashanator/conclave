import { describe, expect, test } from 'bun:test';

const { parseStoredTitle } = await import('../../src/lib/calendar-title');

describe('parseStoredTitle', () => {
  test('a calendar origin comes back with its uid and start', () => {
    const raw = JSON.stringify({ source: 'calendar', calendar: { uid: 'uid-1', start: '2026-10-22T09:00:00+00:00' } });
    expect(parseStoredTitle(raw)).toEqual({ source: 'calendar', calendar: { uid: 'uid-1', start: '2026-10-22T09:00:00+00:00' } });
  });

  test('nothing stored, or a broken value, gives null', () => {
    expect(parseStoredTitle(null)).toBeNull();
    expect(parseStoredTitle('{not json')).toBeNull();
  });

  test('an unknown source, or a calendar origin without its link, is a default title', () => {
    expect(parseStoredTitle(JSON.stringify({ source: 'bogus', calendar: null }))).toEqual({ source: 'default', calendar: null });
    expect(parseStoredTitle(JSON.stringify({ source: 'calendar', calendar: null }))).toEqual({ source: 'default', calendar: null });
  });
});
