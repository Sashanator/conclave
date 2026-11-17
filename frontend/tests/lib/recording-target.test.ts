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
const toast = { warning: mock((_w: string, _o?: unknown) => {}), error: mock((_w: string, _o?: unknown) => {}), info: mock(() => {}), success: mock(() => {}) };
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('sonner', () => ({ toast }));
const { routeRecordingStart, takeAutoStartMeetingId, routeOrFallback, releaseFailedStart, queueAutoStart } = await import('../../src/lib/recording-target');

beforeEach(() => { invoke.mockClear(); toast.warning.mockClear(); toast.error.mockClear(); routeResult = async () => ({ meetingId: 'm', title: 'T' }); });

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
    expect(r.title).toBeNull(); // Rust then names it "Untitled · day time"
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

describe('routeOrFallback id hygiene', () => {
  test('a new start leaves last_recording_meeting_id alone: the previous recording may still be saving', async () => {
    sessionStorage.setItem('last_recording_meeting_id', 'prev');
    await routeOrFallback('sidebar');
    expect(sessionStorage.getItem('last_recording_meeting_id')).toBe('prev');
  });
});

describe('releaseFailedStart', () => {
  test('releases the routed row', async () => {
    routeResult = async () => true;
    await releaseFailedStart({ meetingId: 'm9', title: 'T' }, new Error('no device'));
    expect(invoke).toHaveBeenCalledWith('api_release_routed_start', { meetingId: 'm9' });
  });
  test('skips when nothing was routed', async () => {
    await releaseFailedStart({ meetingId: null, title: null }, new Error('x'));
    await releaseFailedStart(null, new Error('x'));
    expect(invoke).not.toHaveBeenCalled();
  });
  test('a lost race still releases: Rust refuses the winner\'s row', async () => {
    routeResult = async () => false;
    await releaseFailedStart({ meetingId: 'm9', title: 'T' }, 'Recording already in progress');
    expect(invoke).toHaveBeenCalledWith('api_release_routed_start', { meetingId: 'm9' });
  });
  test('toasts when the release fails', async () => {
    routeResult = async () => { throw 'db locked'; };
    const logged = console.error; console.error = () => {};
    await releaseFailedStart({ meetingId: 'm9', title: 'T' }, new Error('x'));
    console.error = logged;
    expect(toast.error).toHaveBeenCalledTimes(1);
  });
});

describe('queueAutoStart', () => {
  test('sets both keys and returns the meeting page url', () => {
    sessionStorage.clear();
    expect(queueAutoStart(sessionStorage, 'm 1')).toBe('/meeting-details?id=m%201');
    expect(sessionStorage.getItem('autoStartMeetingId')).toBe('m 1');
    expect(sessionStorage.getItem('autoStartRecording')).toBe('true');
  });
});
