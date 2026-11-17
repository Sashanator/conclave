import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  sonner: { ...await import('sonner') },
  event: { ...await import('@tauri-apps/api/event') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
  mock.module('@tauri-apps/api/event', () => originals.event);
});

let routeResult: () => Promise<unknown> = async () => ({ meetingId: 'm', title: 'T' });
const invoke = mock(async (_c: string, _a?: unknown) => routeResult());
const toast = { warning: mock((_w: string, _o?: unknown) => {}), error: mock((_w: string, _o?: unknown) => {}), info: mock(() => {}), success: mock(() => {}) };
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('sonner', () => ({ toast }));
let eventHandler: ((e: { payload: string }) => void) | null = null;
mock.module('@tauri-apps/api/event', () => ({
  ...originals.event,
  listen: mock(async (_name: string, h: (e: { payload: string }) => void) => { eventHandler = h; return () => {}; }),
}));
const { routeRecordingStart, takeAutoStartMeetingId, routeOrFallback, releaseFailedStart, queueAutoStart, listenReleaseFailed } = await import('../../src/lib/recording-target');

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
    const url = queueAutoStart(sessionStorage, 'm 1');
    expect(url).toMatch(/^\/meeting-details\?id=m%201&start=\d+$/);
    expect(sessionStorage.getItem('autoStartKey')).toBe(`m 1:${url.split('start=')[1]}`);
    expect(sessionStorage.getItem('autoStartMeetingId')).toBe('m 1');
    expect(sessionStorage.getItem('autoStartRecording')).toBe('true');
  });
});

describe('queueAutoStart without a row', () => {
  test('queues only the flag and returns the id-less page', () => {
    sessionStorage.clear();
    expect(queueAutoStart(sessionStorage, null)).toBe('/meeting-details');
    expect(sessionStorage.getItem('autoStartRecording')).toBe('true');
    expect(sessionStorage.getItem('autoStartMeetingId')).toBeNull();
  });
});

describe('showRecordingView', () => {
  const { showRecordingView } = require('../../src/lib/recording-target');
  const base = { meetingId: 'm1', liveMeetingId: null, heldId: null, stopFlow: false };
  test('live here, held through the save, and no hold once the save is over', () => {
    expect(showRecordingView({ ...base, liveMeetingId: 'm1' })).toBe(true);
    expect(showRecordingView({ ...base, heldId: 'm1', stopFlow: true })).toBe(true);
    expect(showRecordingView({ ...base, heldId: 'm1', stopFlow: false })).toBe(false);
    expect(showRecordingView({ ...base, heldId: 'm2', stopFlow: true })).toBe(false);
    expect(showRecordingView({ ...base, meetingId: null, liveMeetingId: null })).toBe(false);
  });
});

describe('autoStartAllowed', () => {
  const { autoStartAllowed, pageKeyFor } = require('../../src/lib/recording-target');
  test('no key: any page may start; a key: only its page, only while fresh', () => {
    sessionStorage.clear();
    expect(autoStartAllowed(sessionStorage, undefined)).toBe(true);
    sessionStorage.setItem('autoStartKey', 'a:1000');
    sessionStorage.setItem('autoStartRecording', 'true');
    expect(autoStartAllowed(sessionStorage, pageKeyFor('b', null), 2000)).toBe(false);
    expect(autoStartAllowed(sessionStorage, pageKeyFor('a', '1000'), 2000)).toBe(true);
    expect(sessionStorage.getItem('autoStartRecording')).toBe('true');
    // stale: dropped for good, flags included
    expect(autoStartAllowed(sessionStorage, pageKeyFor('a', '1000'), 1000 + 60_000)).toBe(false);
    expect(sessionStorage.getItem('autoStartKey')).toBeNull();
    expect(sessionStorage.getItem('autoStartRecording')).toBeNull();
  });
});

describe('listenReleaseFailed', () => {
  test('a tray release failure shows the left-behind toast', async () => {
    await listenReleaseFailed();
    eventHandler!({ payload: 'db locked' });
    expect(toast.error).toHaveBeenCalledWith('An empty meeting was left behind', {
      description: 'db locked Delete the empty started meeting by hand.',
    });
  });
});
