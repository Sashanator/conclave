import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingNote } from '../../src/types';

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
mock.module('@tauri-apps/api/core', () => ({ ...originalCore, invoke }));
const toastError = mock((_message: string, _options?: unknown) => {});
mock.module('sonner', () => ({ toast: { error: toastError } }));
const { useMeetingNotes } = await import('../../src/hooks/useMeetingNotes');
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originalCore);
});

let state: ReturnType<typeof useMeetingNotes>;
let renderer: ReactTestRenderer | undefined;
function View({ meetingId }: { meetingId: string }) {
  state = useMeetingNotes(meetingId);
  return <output>{state.loaded ? 'loaded' : 'loading'}:{state.notes.map(n => n.text).join(',')}</output>;
}
// The save queue outlives a mount (by design), so every test gets its own meeting.
let meetingId = '';
let meetingSeq = 0;
beforeEach(() => {
  requests.length = 0;
  invoke.mockClear();
  toastError.mockClear();
  meetingId = `m${++meetingSeq}`;
});
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
});

const old: MeetingNote = { id: 'n1', at: 3, text: 'old' };
const pending = (command: string) => requests.filter(r => r.command === command);
async function resolve(req: Request, value: unknown) {
  await act(async () => { req.resolve(value); });
}
async function reject(req: Request, reason: unknown) {
  await act(async () => { req.reject(reason); });
}
async function mountLoaded(notes: MeetingNote[] = [old]) {
  await act(async () => { renderer = create(<View meetingId={meetingId} />); });
  await resolve(pending('get_meeting_notes')[0], notes);
}

describe('useMeetingNotes', () => {
  test('edits are ignored until the notes are loaded', async () => {
    await act(async () => { renderer = create(<View meetingId={meetingId} />); });
    expect(state.loaded).toBe(false);
    await act(async () => { state.addNote('x'); });
    expect(pending('save_meeting_notes')).toHaveLength(0);
    await resolve(pending('get_meeting_notes')[0], [old]);
    expect(state.loaded).toBe(true);
  });

  test('a new note is saved after the stored ones', async () => {
    await mountLoaded();
    await act(async () => { state.addNote('new'); });
    const save = pending('save_meeting_notes')[0];
    expect(save.args.meetingId).toBe(meetingId);
    const saved = save.args.notes as MeetingNote[];
    expect(saved.map(n => n.text)).toEqual(['old', 'new']);
    expect(saved[0]).toEqual(old);
    expect(saved[1].at).toBeNull();
    await resolve(save, saved);
    expect(state.notes.map(n => n.text)).toEqual(['old', 'new']);
  });

  test('two quick edits save one after the other and the UI ends with the second list', async () => {
    await mountLoaded();
    await act(async () => { state.addNote('a'); state.addNote('b'); });
    expect(pending('save_meeting_notes')).toHaveLength(1);
    const first = pending('save_meeting_notes')[0];
    await resolve(first, first.args.notes);
    expect(pending('save_meeting_notes')).toHaveLength(2);
    const second = pending('save_meeting_notes')[1];
    expect((second.args.notes as MeetingNote[]).map(n => n.text)).toEqual(['old', 'a', 'b']);
    await resolve(second, second.args.notes);
    expect(state.notes.map(n => n.text)).toEqual(['old', 'a', 'b']);
  });

  test('a successful save then a failed one shows the successful list', async () => {
    await mountLoaded();
    await act(async () => { state.addNote('a'); state.addNote('b'); });
    const first = pending('save_meeting_notes')[0];
    await resolve(first, first.args.notes);
    const second = pending('save_meeting_notes')[1];
    await reject(second, new Error('disk full'));
    expect(state.notes.map(n => n.text)).toEqual(['old', 'a']);
    expect(toastError).toHaveBeenCalledTimes(1);
  });

  test('a load started on a remount waits for a save still in flight, so it reads the saved list', async () => {
    await mountLoaded();
    await act(async () => { state.addNote('new'); });
    const save = pending('save_meeting_notes')[0];
    // Leave the meeting and reopen it while the save is still running.
    await act(async () => renderer!.unmount());
    renderer = undefined;
    await act(async () => { renderer = create(<View meetingId={meetingId} />); });
    expect(pending('get_meeting_notes')).toHaveLength(1);
    expect(state.loaded).toBe(false);
    await resolve(save, save.args.notes);
    expect(pending('get_meeting_notes')).toHaveLength(2);
    await resolve(pending('get_meeting_notes')[1], save.args.notes);
    expect(state.loaded).toBe(true);
    expect(state.notes.map(n => n.text)).toEqual(['old', 'new']);
  });

  test('a rejected save restores the last confirmed list', async () => {
    await mountLoaded();
    await act(async () => { state.updateNote('n1', 'changed'); });
    await reject(pending('save_meeting_notes')[0], new Error('locked'));
    expect(state.notes.map(n => n.text)).toEqual(['old']);
    expect(toastError).toHaveBeenCalledTimes(1);
  });
});
