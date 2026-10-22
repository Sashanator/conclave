import { afterAll, describe, expect, mock, test } from 'bun:test';

const originals = { core: { ...(await import('@tauri-apps/api/core')) } };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
});

const invoke = mock(async (_command: string): Promise<unknown> => null);
mock.module('@tauri-apps/api/core', () => ({ invoke }));

const { pickStartTitle } = await import('../../src/lib/calendar-title');

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
