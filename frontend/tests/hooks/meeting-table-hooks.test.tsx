import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingRow, MeetingView } from '../../src/lib/meetings';

const originalCore = { ...await import('@tauri-apps/api/core') };
const originalEvent = { ...await import('@tauri-apps/api/event') };
const originalSonner = { ...await import('sonner') };
const originalRec = { ...await import('../../src/contexts/RecordingStateContext') };
const originalSidebar = { ...await import('../../src/components/Sidebar/SidebarProvider') };

type Request = { command: string; args: Record<string, unknown>; resolve: (v: unknown) => void; reject: (r: unknown) => void };
const requests: Request[] = [];
const invoke = mock((command: string, args: Record<string, unknown>) => new Promise((resolve, reject) => {
  requests.push({ command, args, resolve, reject });
}));
let listenImpl: (...a: unknown[]) => Promise<() => void> = async () => () => {};
const toastError = mock((_m: string, _o?: unknown) => {});
const toastWarning = mock((_m: string, _o?: unknown) => {});
const refetchMeetings = mock(async () => {});
mock.module('@tauri-apps/api/core', () => ({ ...originalCore, invoke }));
mock.module('@tauri-apps/api/event', () => ({ ...originalEvent, listen: (...a: unknown[]) => listenImpl(...a) }));
mock.module('sonner', () => ({ toast: { error: toastError, warning: toastWarning } }));
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originalRec, useRecordingState: () => ({ liveMeetingId: 'live1' }) }));
mock.module('../../src/components/Sidebar/SidebarProvider', () => ({ ...originalSidebar, useSidebar: () => ({ refetchMeetings }) }));
const { useMeetingRows } = await import('../../src/hooks/useMeetingRows');
const { useMeetingViews } = await import('../../src/hooks/useMeetingViews');
afterAll(() => {
  mock.module('sonner', () => originalSonner);
  mock.module('@tauri-apps/api/core', () => originalCore);
  mock.module('@tauri-apps/api/event', () => originalEvent);
  mock.module('../../src/contexts/RecordingStateContext', () => originalRec);
  mock.module('../../src/components/Sidebar/SidebarProvider', () => originalSidebar);
});

const row = (id: string): MeetingRow => ({
  id, title: id, titleSource: 'user', templateId: null, emoji: null, reviewed: false, gist: null,
  startedAt: null, endedAt: null, createdAt: '2026-11-16 10:00:00+00:00', calendarUid: null,
  calendarStart: null, calendarEnd: null, calendarCancelledAt: null, hasTranscript: false, hasSummary: false, prepPoints: 0,
});
const view = (id: string, position: number, builtin: boolean): MeetingView => ({ id, name: id, position, builtin, configJson: '{}' });

let rowsApi: ReturnType<typeof useMeetingRows>;
let viewsApi: ReturnType<typeof useMeetingViews>;
let renderer: ReactTestRenderer | undefined;
function Rows() { rowsApi = useMeetingRows(); return <output>{rowsApi.rows.map(r => `${r.id}:${r.reviewed}`).join(',')}</output>; }
function Views() { viewsApi = useMeetingViews(); return null; }
const pending = (c: string) => requests.filter(r => r.command === c);
const settle = async (r: Request, ok: boolean, v: unknown = null) => { await act(async () => { ok ? r.resolve(v) : r.reject(new Error('boom')); }); };

beforeEach(() => { listenImpl = async () => () => {}; requests.length = 0; invoke.mockClear(); toastError.mockClear(); toastWarning.mockClear(); refetchMeetings.mockClear(); try { localStorage.clear(); } catch {} });
afterEach(async () => { if (renderer) await act(async () => renderer!.unmount()); renderer = undefined; });

async function mountRows() {
  await act(async () => { renderer = create(<Rows />); });
  await settle(pending('api_list_meeting_rows')[0], true, [row('a'), row('b')]);
}
async function mountViews() {
  await act(async () => { renderer = create(<Views />); });
  await settle(pending('api_list_meeting_views')[0], true, [view('all', 0, true), view('week', 1, true), view('mine', 2, false)]);
}

describe('useMeetingRows', () => {
  test('loads rows and exposes the live id', async () => {
    await mountRows();
    expect(rowsApi.loaded).toBe(true);
    expect(rowsApi.rows.map(r => r.id)).toEqual(['a', 'b']);
    expect(rowsApi.liveId).toBe('live1');
  });
  test('toggleReviewed flips at once and keeps the flip on success', async () => {
    await mountRows();
    let p: Promise<void>;
    await act(async () => { p = rowsApi.toggleReviewed('a'); });
    expect(rowsApi.rows[0].reviewed).toBe(true);
    expect(pending('api_set_meeting_reviewed')[0].args).toEqual({ meetingId: 'a', reviewed: true });
    await settle(pending('api_set_meeting_reviewed')[0], true);
    await p!;
    expect(rowsApi.rows[0].reviewed).toBe(true);
  });
  test('toggleReviewed rolls back and toasts on reject', async () => {
    await mountRows();
    let p: Promise<void>;
    await act(async () => { p = rowsApi.toggleReviewed('a'); });
    await settle(pending('api_set_meeting_reviewed')[0], false);
    await p!;
    expect(rowsApi.rows[0].reviewed).toBe(false);
    expect(toastError.mock.calls[0][0]).toBe('Could not save Reviewed');
  });
  test('removeMeeting drops the row, refetches the sidebar and warns about a kept folder', async () => {
    await mountRows();
    let p: Promise<void>;
    await act(async () => { p = rowsApi.removeMeeting('a'); });
    expect(pending('api_delete_meeting')[0].args).toEqual({ meetingId: 'a' });
    await settle(pending('api_delete_meeting')[0], true, { folder_warning: 'kept' });
    await p!;
    expect(rowsApi.rows.map(r => r.id)).toEqual(['b']);
    expect(refetchMeetings).toHaveBeenCalledTimes(1);
    expect(toastWarning.mock.calls[0][0]).toBe('The meeting was deleted, but its folder was kept');
  });
  test('a rejected delete toasts and keeps the row', async () => {
    await mountRows();
    let p: Promise<void>;
    await act(async () => { p = rowsApi.removeMeeting('a'); });
    await settle(pending('api_delete_meeting')[0], false);
    await p!;
    expect(rowsApi.rows.map(r => r.id)).toEqual(['a', 'b']);
    expect(toastError).toHaveBeenCalledTimes(1);
    expect(refetchMeetings).not.toHaveBeenCalled();
  });
});

describe('useMeetingViews', () => {
  test('setGroup saves once, debounced', async () => {
    await mountViews();
    await act(async () => { viewsApi.dispatch({ type: 'setGroup', group: 'none' }); viewsApi.dispatch({ type: 'setGroup', group: 'day' }); });
    expect(pending('api_save_meeting_view')).toHaveLength(0);
    await act(async () => { await new Promise(r => setTimeout(r, 600)); });
    const saves = pending('api_save_meeting_view');
    expect(saves).toHaveLength(1);
    expect(JSON.parse((saves[0].args.view as MeetingView).configJson).group).toBe('day');
  });
  test('a rejected view save toasts', async () => {
    await mountViews();
    await act(async () => { viewsApi.dispatch({ type: 'setGroup', group: 'none' }); });
    await act(async () => { await new Promise(r => setTimeout(r, 600)); });
    await settle(pending('api_save_meeting_view')[0], false);
    expect(toastError.mock.calls[0][0]).toBe('Could not save the view');
  });
  test('addView saves a custom view with the active config and activates it', async () => {
    await mountViews();
    await act(async () => { void viewsApi.addView(); });
    const saved = pending('api_save_meeting_view')[0].args.view as MeetingView;
    expect(saved).toMatchObject({ builtin: false, position: 3, name: 'New view' });
    expect(JSON.parse(saved.configJson)).toEqual(viewsApi.config);
    expect(viewsApi.active?.id).toBe(saved.id);
    await settle(pending('api_save_meeting_view')[0], true);
  });
  test('removeView on a builtin makes no call', async () => {
    await mountViews();
    await act(async () => { await viewsApi.removeView('all'); });
    expect(pending('api_delete_meeting_view')).toHaveLength(0);
  });
});

const cfg = (v: MeetingView | undefined) => JSON.parse(v!.configJson);
const wait = (ms: number) => act(async () => { await new Promise(r => setTimeout(r, ms)); });

describe('useMeetingRows fixes', () => {
  test('a stale reload reply is dropped', async () => {
    await mountRows();
    await act(async () => { void rowsApi.reload(); void rowsApi.reload(); });
    const [first, second] = pending('api_list_meeting_rows').slice(1);
    await settle(second, true, [row('new')]);
    await settle(first, true, [row('old')]);
    expect(rowsApi.rows.map(r => r.id)).toEqual(['new']);
  });
  test('a failed first load exposes error, toasts once and a retry clears it', async () => {
    await act(async () => { renderer = create(<Rows />); });
    await settle(pending('api_list_meeting_rows')[0], false);
    expect(rowsApi.loaded).toBe(false);
    expect(rowsApi.error).toBe(true);
    await act(async () => { void rowsApi.reload(); });
    await settle(pending('api_list_meeting_rows')[1], false);
    expect(toastError).toHaveBeenCalledTimes(1);
    await act(async () => { void rowsApi.reload(); });
    await settle(pending('api_list_meeting_rows')[2], true, [row('a')]);
    expect(rowsApi.error).toBe(false);
    expect(rowsApi.loaded).toBe(true);
  });
  test('a rejected listen does not break mount or unmount', async () => {
    const spy = mock((..._a: unknown[]) => {});
    const orig = console.error;
    console.error = spy;
    listenImpl = async () => { throw new Error('no events'); };
    await mountRows();
    await act(async () => renderer!.unmount());
    renderer = undefined;
    await wait(0);
    console.error = orig;
    expect(spy).toHaveBeenCalled();
  });
  test('an older reload cannot undo a later toggle', async () => {
    await mountRows();
    await act(async () => { void rowsApi.reload(); });
    const stale = pending('api_list_meeting_rows')[1];
    await act(async () => { void rowsApi.toggleReviewed('a'); });
    await settle(pending('api_set_meeting_reviewed')[0], true);
    await settle(stale, true, [row('a'), row('b')]); // the old snapshot still says unreviewed
    expect(rowsApi.rows[0].reviewed).toBe(true);
  });
  test('changeTemplate rolls back and toasts on failure', async () => {
    await mountRows();
    let p: Promise<void>;
    await act(async () => { p = rowsApi.changeTemplate('a', 'interview'); });
    expect(rowsApi.rows[0].templateId).toBe('interview');
    await settle(pending('api_set_meeting_template')[0], false);
    await p!;
    expect(rowsApi.rows[0].templateId).toBeNull();
    expect(toastError.mock.calls[0][0]).toBe('Could not save the template');
  });
});

describe('useMeetingViews fixes', () => {
  test('removeView cancels a queued save for that view', async () => {
    await mountViews();
    await act(async () => { viewsApi.setActive('mine'); });
    await act(async () => { viewsApi.dispatch({ type: 'setGroup', group: 'day' }); });
    await act(async () => { void viewsApi.removeView('mine'); });
    await wait(600);
    expect(pending('api_save_meeting_view')).toHaveLength(0);
    expect(pending('api_delete_meeting_view')).toHaveLength(1);
    await settle(pending('api_delete_meeting_view')[0], true);
  });
  test('renameView sends the queued save first', async () => {
    await mountViews();
    await act(async () => { viewsApi.setActive('mine'); });
    await act(async () => { viewsApi.dispatch({ type: 'setGroup', group: 'day' }); });
    await act(async () => { void viewsApi.renameView('mine', 'Renamed'); });
    // The rename waits until the queued edit has landed.
    let saves = pending('api_save_meeting_view').map(r => r.args.view as MeetingView);
    expect(saves).toHaveLength(1);
    expect(saves[0].name).toBe('mine');
    expect(cfg(saves[0]).group).toBe('day');
    await settle(pending('api_save_meeting_view')[0], true);
    saves = pending('api_save_meeting_view').map(r => r.args.view as MeetingView);
    expect(saves).toHaveLength(2);
    expect(saves[1].name).toBe('Renamed');
    await wait(600);
    expect(pending('api_save_meeting_view')).toHaveLength(2);
    await settle(pending('api_save_meeting_view')[1], true);
  });
  test('removeView waits for a save still in flight', async () => {
    await mountViews();
    await act(async () => { viewsApi.setActive('mine'); });
    await act(async () => { viewsApi.dispatch({ type: 'setGroup', group: 'day' }); });
    await wait(600); // the save is now on its way
    await act(async () => { void viewsApi.removeView('mine'); });
    expect(pending('api_delete_meeting_view')).toHaveLength(0);
    await settle(pending('api_save_meeting_view')[0], true);
    expect(pending('api_delete_meeting_view')).toHaveLength(1);
    await settle(pending('api_delete_meeting_view')[0], true);
  });
  test('a failed removeView queues the unsaved edit again', async () => {
    await mountViews();
    await act(async () => { viewsApi.setActive('mine'); });
    await act(async () => { viewsApi.dispatch({ type: 'setGroup', group: 'day' }); });
    await act(async () => { void viewsApi.removeView('mine'); });
    await settle(pending('api_delete_meeting_view')[0], false);
    expect(pending('api_save_meeting_view')).toHaveLength(0);
    await wait(600);
    expect(pending('api_save_meeting_view')).toHaveLength(1);
    expect(cfg(pending('api_save_meeting_view')[0].args.view as MeetingView).group).toBe('day');
    await settle(pending('api_save_meeting_view')[0], true);
  });
  test('two dispatches in one handler both apply', async () => {
    await mountViews();
    await act(async () => {
      viewsApi.dispatch({ type: 'setGroup', group: 'day' });
      viewsApi.dispatch({ type: 'setDensity', density: 'compact' });
    });
    expect(viewsApi.config.group).toBe('day');
    expect(viewsApi.config.density).toBe('compact');
    await wait(600);
    const saved = cfg(pending('api_save_meeting_view')[0].args.view as MeetingView);
    expect(saved.group).toBe('day');
    expect(saved.density).toBe('compact');
  });
  test('a failed debounced save rolls back to the last saved config', async () => {
    await mountViews();
    const before = viewsApi.config.group;
    await act(async () => { viewsApi.dispatch({ type: 'setGroup', group: before === 'day' ? 'none' : 'day' }); });
    expect(viewsApi.config.group).not.toBe(before);
    await wait(600);
    await settle(pending('api_save_meeting_view')[0], false);
    expect(viewsApi.config.group).toBe(before);
    expect(toastError.mock.calls[0][0]).toBe('Could not save the view');
  });
  test('unmount flushes the queued edit', async () => {
    await mountViews();
    await act(async () => { viewsApi.dispatch({ type: 'setGroup', group: 'day' }); });
    await act(async () => renderer!.unmount());
    renderer = undefined;
    expect(pending('api_save_meeting_view')).toHaveLength(1);
  });
  test('addView failure rolls back and re-activates the previous view', async () => {
    await mountViews();
    const prev = viewsApi.active!.id;
    await act(async () => { void viewsApi.addView(); });
    await settle(pending('api_save_meeting_view')[0], false);
    expect(viewsApi.views.map(v => v.id)).not.toContain('New view');
    expect(viewsApi.views).toHaveLength(3);
    expect(viewsApi.active?.id).toBe(prev);
    expect(toastError).toHaveBeenCalledTimes(1);
  });
  test('renameView failure restores the old name', async () => {
    await mountViews();
    await act(async () => { void viewsApi.renameView('mine', 'Renamed'); });
    expect(viewsApi.views.find(v => v.id === 'mine')?.name).toBe('Renamed');
    await settle(pending('api_save_meeting_view')[0], false);
    expect(viewsApi.views.find(v => v.id === 'mine')?.name).toBe('mine');
    expect(toastError).toHaveBeenCalledTimes(1);
  });
});
