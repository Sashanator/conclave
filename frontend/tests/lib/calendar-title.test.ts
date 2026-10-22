import { afterAll, describe, expect, mock, test } from 'bun:test';

const originals = { core: { ...(await import('@tauri-apps/api/core')) } };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
});

const invoke = mock(async (_command: string): Promise<unknown> => null);
mock.module('@tauri-apps/api/core', () => ({ invoke }));

const { parseStoredTitle, pickStartTitle } = await import('../../src/lib/calendar-title');

describe('pickStartTitle', () => {
  test('an event now names the recording and is a calendar title', async () => {
    invoke.mockImplementationOnce(async () => ({ title: 'Design review', uid: 'uid-1', start: '2026-10-22T09:00:00+00:00' }));
    expect(await pickStartTitle('Generated name')).toEqual({
      title: 'Design review',
      source: 'calendar',
      uid: 'uid-1',
      start: '2026-10-22T09:00:00+00:00',
    });
    expect(invoke).toHaveBeenCalledWith('calendar_title_now');
  });

  test('an event with a blank title keeps the generated title as a default', async () => {
    invoke.mockImplementationOnce(async () => ({ title: '   ', uid: 'uid-2', start: '2026-10-22T09:00:00+00:00' }));
    expect(await pickStartTitle('Generated name')).toEqual({ title: 'Generated name', source: 'default' });
  });

  test('no event keeps the generated title as a default', async () => {
    invoke.mockImplementationOnce(async () => null);
    expect(await pickStartTitle('Generated name')).toEqual({ title: 'Generated name', source: 'default' });
  });

  test('a failing calendar keeps the generated title as a default', async () => {
    invoke.mockImplementationOnce(async () => {
      throw new Error('calendar unavailable');
    });
    expect(await pickStartTitle('Generated name')).toEqual({ title: 'Generated name', source: 'default' });
  });
});

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
