import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingAudioInfo } from '../../src/lib/meeting-audio';

const originalCore = { ...await import('@tauri-apps/api/core') };
type Request = {
  command: string;
  args: Record<string, unknown>;
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
};
const requests: Request[] = [];
const invoke = mock((command: string, args: Record<string, unknown>) => new Promise((resolve, reject) => {
  requests.push({ command, args, resolve, reject });
}));
mock.module('@tauri-apps/api/core', () => ({ ...originalCore, invoke, convertFileSrc: (path: string) => `asset://${path}` }));
const { AUDIO_DELETED_EVENT } = await import('../../src/lib/meeting-audio');
const { useMeetingAudio } = await import('../../src/hooks/useMeetingAudio');
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originalCore);
});

let audio: ReturnType<typeof useMeetingAudio>;
let renderer: ReactTestRenderer | undefined;
function View({ meetingId }: { meetingId: string }) {
  audio = useMeetingAudio(meetingId);
  return null;
}
beforeEach(() => {
  requests.length = 0;
  invoke.mockClear();
});
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
});

const lookups = () => requests.filter(r => r.command === 'get_meeting_audio');
async function resolve(req: Request, value: unknown) {
  await act(async () => { req.resolve(value); });
}
const found: MeetingAudioInfo = { path: 'C:\\rec\\m1\\audio.mp4', keep_audio: false, audio_deleted_at: null };
const deleted: MeetingAudioInfo = { path: null, keep_audio: false, audio_deleted_at: '2026-11-20T10:00:00+00:00' };
async function mountWith(info: MeetingAudioInfo) {
  await act(async () => { renderer = create(<View meetingId="m1" />); });
  await resolve(lookups()[0], info);
  expect(audio.info?.path).toBe(info.path);
}
function notify() {
  act(() => { window.dispatchEvent(new Event(AUDIO_DELETED_EVENT)); });
}

describe('useMeetingAudio', () => {
  test('only the newest lookup applies: an older reply cannot bring back a deleted path', async () => {
    await mountWith(found);
    notify();
    notify();
    expect(lookups()).toHaveLength(3);
    await resolve(lookups()[2], deleted);
    await resolve(lookups()[1], found); // the older lookup answers last
    expect(audio.info?.path).toBeNull();
    expect(audio.info?.audio_deleted_at).toBe(deleted.audio_deleted_at);
  });

  test('when the file goes away during playback, the playhead and play state reset', async () => {
    await mountWith(found);
    act(() => audio.audioEvents.onPlay());
    act(() => audio.audioEvents.onTimeUpdate({ currentTarget: { currentTime: 12 } } as never));
    expect(audio.isPlaying).toBe(true);
    expect(audio.playhead).toBe(12);
    notify();
    await resolve(lookups()[1], deleted);
    expect(audio.playhead).toBeNull();
    expect(audio.isPlaying).toBe(false);
  });
});
