import { afterAll, afterEach, describe, expect, mock, spyOn, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingSpeakers } from '../../src/types';

const originalCore = { ...await import('@tauri-apps/api/core') };
const originalEvent = { ...await import('@tauri-apps/api/event') };
const originalToast = { ...await import('sonner') };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originalCore);
  mock.module('@tauri-apps/api/event', () => originalEvent);
  mock.module('sonner', () => originalToast);
});

type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();
let meetingReply: MeetingSpeakers = { job: null, speakers: [] };
// When set, each speakers_get_meeting waits for its own deferred reply
let heldReplies: Array<{ resolve: (v: MeetingSpeakers) => void; promise: Promise<MeetingSpeakers> }> | null = null;
let failProgressListen = false;
const invoke = mock(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'speakers_name') return 'anya-id';
  if (command === 'speakers_get_meeting') {
    if (heldReplies) {
      let resolve!: (v: MeetingSpeakers) => void;
      const promise = new Promise<MeetingSpeakers>((r) => { resolve = r; });
      heldReplies.push({ resolve, promise });
      return promise;
    }
    return meetingReply;
  }
  return undefined;
});
mock.module('@tauri-apps/api/core', () => ({ ...originalCore, invoke }));
mock.module('@tauri-apps/api/event', () => ({
  ...originalEvent,
  listen: async (event: string, handler: Handler) => {
    if (event === 'speakers-progress' && failProgressListen) throw new Error('listen refused');
    handlers.set(event, handler);
    return () => handlers.delete(event);
  },
}));
const toastError = mock(() => {});
mock.module('sonner', () => ({ toast: { error: toastError, success: toastError, info: toastError, warning: toastError } }));
const { useMeetingSpeakers } = await import('../../src/hooks/useMeetingSpeakers');

type Hook = ReturnType<typeof useMeetingSpeakers>;
let latest: Hook;
type ProbeProps = { meetingId: string; active: boolean; onChanged?: () => Promise<void>; onLineMoved?: (id: string, idx: number | null) => void };
function Probe(props: ProbeProps) {
  latest = useMeetingSpeakers(props.meetingId, props.active, props.onChanged, props.onLineMoved);
  return null;
}

let renderer: ReactTestRenderer | undefined;
async function mount(props: ProbeProps) {
  await act(async () => { renderer = create(<Probe {...props} />); });
}
const calls = (command: string) => invoke.mock.calls.filter(([c]) => c === command);
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  invoke.mockClear();
  handlers.clear();
  meetingReply = { job: null, speakers: [] };
  heldReplies = null;
  failProgressListen = false;
});

describe('useMeetingSpeakers', () => {
  test('inactive: no commands, no events, loaded at once', async () => {
    await mount({ meetingId: 'm1', active: false });
    expect(invoke).not.toHaveBeenCalled();
    expect(handlers.size).toBe(0);
    expect(latest.loaded).toBe(true);
    expect(latest.busy).toBe(false);
  });

  test('active: loads the meeting and reports busy with the percent', async () => {
    meetingReply = { job: { status: 'running', reason: null, percent: 40 }, speakers: [] };
    await mount({ meetingId: 'm1', active: true });
    expect(calls('speakers_get_meeting')[0][1]).toEqual({ meetingId: 'm1' });
    expect(latest.loaded).toBe(true);
    expect(latest.busy).toBe(true);
    expect(latest.percent).toBe(40);
  });

  test('speakers-updated for another meeting is ignored', async () => {
    const onChanged = mock(async () => {});
    await mount({ meetingId: 'm1', active: true, onChanged });
    invoke.mockClear();
    await act(async () => { handlers.get('speakers-updated')!({ payload: { meeting_id: 'other' } }); });
    expect(invoke).not.toHaveBeenCalled();
    expect(onChanged).not.toHaveBeenCalled();
  });

  test('speakers-updated for this meeting reloads and re-fetches the transcripts', async () => {
    const onChanged = mock(async () => {});
    await mount({ meetingId: 'm1', active: true, onChanged });
    meetingReply = { job: { status: 'done', reason: null, percent: null }, speakers: [] };
    invoke.mockClear();
    await act(async () => { handlers.get('speakers-updated')!({ payload: { meeting_id: 'm1' } }); });
    expect(calls('speakers_get_meeting').length).toBe(1);
    expect(onChanged).toHaveBeenCalledTimes(1);
    expect(latest.data?.job?.status).toBe('done');
  });

  test('speakers-progress sets the percent from the event and does not reload', async () => {
    const onChanged = mock(async () => {});
    await mount({ meetingId: 'm1', active: true, onChanged });
    invoke.mockClear();
    await act(async () => { handlers.get('speakers-progress')!({ payload: { meeting_id: 'm1', percent: 75 } }); });
    expect(latest.percent).toBe(75);
    expect(invoke).not.toHaveBeenCalled();
    expect(onChanged).not.toHaveBeenCalled();
  });

  test('progress for another meeting is ignored', async () => {
    await mount({ meetingId: 'm1', active: true });
    await act(async () => { handlers.get('speakers-progress')!({ payload: { meeting_id: 'other', percent: 90 } }); });
    expect(latest.percent).toBeNull();
  });

  test('a late reply to an earlier request does not overwrite a newer one', async () => {
    heldReplies = [];
    await mount({ meetingId: 'm1', active: true });
    await act(async () => { void latest.identify(); });
    const [first, second] = heldReplies!;
    expect(heldReplies!.length).toBe(2);
    await act(async () => {
      second.resolve({ job: { status: 'done', reason: null, percent: null }, speakers: [] });
      await second.promise;
    });
    await act(async () => {
      first.resolve({ job: { status: 'running', reason: null, percent: 10 }, speakers: [] });
      await first.promise;
    });
    expect(latest.data?.job?.status).toBe('done');
    expect(latest.busy).toBe(false);
  });

  test('one failing listen still lets the other be cleaned up', async () => {
    failProgressListen = true;
    const logged = spyOn(console, 'error').mockImplementation(() => {});
    try {
      await mount({ meetingId: 'm1', active: true });
    } finally {
      logged.mockRestore();
    }
    expect(handlers.has('speakers-updated')).toBe(true);
    await act(async () => renderer!.unmount());
    renderer = undefined;
    expect(handlers.has('speakers-updated')).toBe(false);
  });

  test('name sends personId, marks names changed until cleared', async () => {
    await mount({ meetingId: 'm1', active: true });
    await act(async () => { await latest.name(2, { personId: 'p1' }); });
    expect(invoke).toHaveBeenCalledWith('speakers_name', { meetingId: 'm1', idx: 2, personId: 'p1' });
    expect(latest.namesChanged).toBe(true);
    act(() => latest.clearNamesChanged());
    expect(latest.namesChanged).toBe(false);
  });

  test('name sends newName for a new person', async () => {
    await mount({ meetingId: 'm1', active: true });
    await act(async () => { await latest.name(1, { newName: 'Аня' }); });
    expect(invoke).toHaveBeenCalledWith('speakers_name', { meetingId: 'm1', idx: 1, newName: 'Аня' });
  });

  test('moving a line to Unknown sends idx null and patches only that line', async () => {
    const onChanged = mock(async () => {});
    const onLineMoved = mock((_id: string, _idx: number | null) => {});
    await mount({ meetingId: 'm1', active: true, onChanged, onLineMoved });
    await act(async () => { await latest.moveLine('t1', null); });
    expect(invoke).toHaveBeenCalledWith('speakers_move_line', { transcriptId: 't1', idx: null });
    expect(onLineMoved).toHaveBeenCalledWith('t1', null);
    expect(onChanged).not.toHaveBeenCalled();
    expect(latest.namesChanged).toBe(true);
  });

  test('name resolves to the person id it was given', async () => {
    await mount({ meetingId: 'm1', active: true });
    const got: { id?: string | null } = {};
    await act(async () => { got.id = await latest.name(1, { newName: 'Аня' }); });
    expect(got.id).toBe('anya-id');
  });

  test('identify starts the job and reloads', async () => {
    await mount({ meetingId: 'm1', active: true });
    invoke.mockClear();
    await act(async () => { await latest.identify(); });
    expect(invoke).toHaveBeenCalledWith('speakers_identify', { meetingId: 'm1' });
    expect(calls('speakers_get_meeting').length).toBe(1);
  });
});
