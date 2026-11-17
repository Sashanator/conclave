import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  sonner: { ...await import('sonner') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
});

let routeResult: () => Promise<unknown> = async () => ({ meetingId: 'm', title: 'T' });
const invoke = mock(async (_c: string, _a?: unknown) => routeResult());
const toast = { warning: mock((_w: string, _o?: unknown) => {}), error: mock(() => {}), info: mock(() => {}), success: mock(() => {}) };
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('sonner', () => ({ toast }));
const { routeRecordingStart, takeAutoStartMeetingId, routeOrFallback } = await import('../../src/lib/recording-target');

beforeEach(() => { invoke.mockClear(); toast.warning.mockClear(); routeResult = async () => ({ meetingId: 'm', title: 'T' }); });

describe('routeRecordingStart', () => {
  test('sidebar and page sources go to api_route_recording_start', async () => {
    await routeRecordingStart('sidebar');
    await routeRecordingStart({ page: 'm1' });
    expect(invoke).toHaveBeenNthCalledWith(1, 'api_route_recording_start', { source: 'sidebar' });
    expect(invoke).toHaveBeenNthCalledWith(2, 'api_route_recording_start', { source: { page: 'm1' } });
  });
});

describe('routeOrFallback', () => {
  test('returns the routed meeting', async () => {
    expect(await routeOrFallback('sidebar')).toEqual({ meetingId: 'm', title: 'T' });
    expect(toast.warning).not.toHaveBeenCalled();
  });
  test('a routing error starts with no meeting id and warns', async () => {
    routeResult = async () => { throw 'This meeting no longer exists'; };
    const r = await routeOrFallback({ page: 'gone' });
    expect(r.meetingId).toBeNull();
    expect(toast.warning).toHaveBeenCalledWith('Recording as a new meeting', { description: 'This meeting no longer exists' });
  });
});

describe('takeAutoStartMeetingId', () => {
  test('returns and removes the key', () => {
    sessionStorage.setItem('autoStartMeetingId', 'm9');
    expect(takeAutoStartMeetingId(sessionStorage)).toBe('m9');
    expect(sessionStorage.getItem('autoStartMeetingId')).toBeNull();
    expect(takeAutoStartMeetingId(sessionStorage)).toBeNull();
  });
});
