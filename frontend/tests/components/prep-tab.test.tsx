import { afterAll, afterEach, beforeEach, expect, mock, test } from 'bun:test';
import { useEffect, type ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingPrep } from '../../src/lib/prep';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  sonner: { ...await import('sonner') },
  recording: { ...await import('../../src/contexts/RecordingStateContext') },
  transcripts: { ...await import('../../src/contexts/TranscriptContext') },
  panel: { ...await import('../../src/app/_components/TranscriptPanel') },
  copilot: { ...await import('../../src/components/Copilot/CopilotPane') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.recording);
  mock.module('../../src/contexts/TranscriptContext', () => originals.transcripts);
  mock.module('../../src/app/_components/TranscriptPanel', () => originals.panel);
  mock.module('../../src/components/Copilot/CopilotPane', () => originals.copilot);
});

const fixture = (): MeetingPrep => ({
  id: 7, eventUid: 'event+one@example.com', eventStart: '2026-11-01T10:00:00+01:00',
  eventTitle: 'Interview', templateId: null, material: '', chat: [], approvedAt: null,
  createdAt: '2026-10-29T10:00:00Z', updatedAt: '2026-10-29T10:00:00Z',
  items: [
    { id: 'a', text: 'Ask about team', kind: 'question', ticked: true, byUser: false },
    { id: 'b', text: 'Clarify role', kind: 'clarification', ticked: false, byUser: true },
  ],
});
let result: MeetingPrep | null;
let load: () => Promise<MeetingPrep | null>;
let tick: (args: Record<string, unknown>) => Promise<MeetingPrep>;
const invoke = mock(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'prep_get' || command === 'prep_for_meeting') return load();
  if (command === 'prep_tick') return tick(_args ?? {});
  if (command === 'continue_status') return { offer: null, target: null };
  return undefined;
});
const errorToast = mock((_message: unknown) => {});
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));
mock.module('@tauri-apps/api/event', () => ({ ...originals.event, listen: async () => () => {} }));
mock.module('sonner', () => ({ ...originals.sonner, toast: { ...originals.sonner.toast, error: errorToast } }));
const recording = { isRecording: true, isSaving: false, activeDuration: 12 };
const tx = {
  calendarLink: { uid: fixture().eventUid, start: fixture().eventStart } as { uid: string; start: string } | null,
  notes: [], addNote: mock(() => {}), updateNote: mock(() => {}), deleteNote: mock(() => {}),
};
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals.recording, useRecordingState: () => recording }));
mock.module('../../src/contexts/TranscriptContext', () => ({ ...originals.transcripts, useTranscripts: () => tx }));
mock.module('../../src/app/_components/TranscriptPanel', () => ({ TranscriptPanel: () => null }));
mock.module('../../src/components/Copilot/CopilotPane', () => ({ CopilotPane: () => <div>Copilot body</div> }));

const { LiveTranscriptAndNotes } = await import('../../src/app/_components/LiveTranscriptAndNotes');
const { NotesWithPrep } = await import('../../src/components/Prep/NotesWithPrep');
const { readLiveSideTab, LIVE_SIDE_TAB_KEY } = await import('../../src/contexts/CopilotContext');
const live = () => <LiveTranscriptAndNotes isProcessingStop={false} isStopping={false} showModal={() => {}} />;
const notes = <div>Original notes</div>;
const postCall = (meetingId = 'meeting-1') => <NotesWithPrep meetingId={meetingId} notes={notes} />;
const renderers: ReactTestRenderer[] = [];
async function render(node: ReactNode = live()) {
  let r!: ReactTestRenderer;
  await act(async () => { r = create(node as React.ReactElement); });
  renderers.push(r);
  return r;
}
const textOf = (n: ReactTestInstance): string => n.children.map((c) => typeof c === 'string' ? c : textOf(c)).join('');
const tabs = (r: ReactTestRenderer) => r.root.findAllByProps({ role: 'tab' });
const prepTab = (r: ReactTestRenderer) => tabs(r).find((n) => textOf(n).startsWith('Prep'));
const boxes = (r: ReactTestRenderer) => r.root.findAllByProps({ type: 'checkbox' });
const json = (r: ReactTestRenderer) => JSON.stringify(r.toJSON());
beforeEach(() => {
  localStorage.clear();
  result = fixture();
  load = async () => result;
  tick = async () => ({ ...fixture(), items: fixture().items.map((i) => ({ ...i, ticked: true })) });
  recording.isRecording = true;
  tx.calendarLink = { uid: fixture().eventUid, start: fixture().eventStart };
  invoke.mockClear();
  errorToast.mockClear();
});
afterEach(() => { act(() => { renderers.splice(0).forEach((r) => r.unmount()); }); });

test('no prep, no tab; stored prep falls back to notes without changing storage', async () => {
  result = null;
  localStorage.setItem(LIVE_SIDE_TAB_KEY, 'prep');
  const r = await render();
  expect(invoke).toHaveBeenCalledWith('prep_get', { eventUid: fixture().eventUid, eventStart: fixture().eventStart });
  expect(prepTab(r)).toBeUndefined();
  expect(tabs(r)[0].props['aria-selected']).toBe(true);
  expect(localStorage.getItem(LIVE_SIDE_TAB_KEY)).toBe('prep');
});

test('tab shows the counter, kind tags, draft label and encoded Open in Prep link', async () => {
  const r = await render();
  expect(textOf(prepTab(r)!)).toBe('Prep 1/2');
  await act(async () => { prepTab(r)!.props.onClick(); });
  expect(json(r)).toContain('draft');
  expect(json(r)).toContain('clarification');
  const link = r.root.findAll((n) => n.type === 'a' && textOf(n) === 'Open in Prep')[0];
  expect(link.props.href).toBe(`/prep?${new URLSearchParams({ event: fixture().eventUid, start: fixture().eventStart })}`);
  // Switching tabs keeps the existing note input and checklist mounted.
  const noteInput = r.root.findByProps({ 'aria-label': 'New note' });
  await act(async () => { tabs(r)[0].props.onClick(); });
  expect(r.root.findByProps({ 'aria-label': 'New note' })).toBe(noteInput);
  expect(boxes(r)).toHaveLength(2);
});

test('approved prep has no draft label', async () => {
  result = { ...fixture(), approvedAt: '2026-10-29T10:00:00Z' };
  expect(json(await render())).not.toContain('draft');
});

test('tick persists immediately without optimistic state', async () => {
  let resolve!: (p: MeetingPrep) => void;
  tick = () => new Promise((done) => { resolve = done; });
  const r = await render();
  act(() => { boxes(r)[1].props.onChange(); });
  expect(invoke).toHaveBeenCalledWith('prep_tick', { prepId: 7, itemId: 'b', ticked: true });
  expect(boxes(r)[1].props.checked).toBe(false);
  await act(async () => { resolve({ ...fixture(), items: fixture().items.map((i) => ({ ...i, ticked: true })) }); });
  expect(textOf(prepTab(r)!)).toBe('Prep 2/2');
  expect(boxes(r)[1].props.checked).toBe(true);
});

test('tick failure reverts and shows a toast', async () => {
  tick = async () => { throw new Error('write failed'); };
  const r = await render();
  await act(async () => { boxes(r)[1].props.onChange(); });
  expect(boxes(r)[1].props.checked).toBe(false);
  expect(textOf(prepTab(r)!)).toBe('Prep 1/2');
  expect(errorToast).toHaveBeenCalledTimes(1);
});

test('post-call notes column shows Prep only with a prep', async () => {
  result = null;
  const r = await render(postCall());
  expect(invoke).toHaveBeenCalledWith('prep_for_meeting', { meetingId: 'meeting-1' });
  expect(tabs(r)).toHaveLength(0);
  expect(json(r)).toContain('Original notes');
});

test('post-call prep can be ticked and both bodies stay mounted', async () => {
  const r = await render(postCall());
  expect(tabs(r).map(textOf)).toEqual(['My notes', 'Prep 1/2']);
  await act(async () => { prepTab(r)!.props.onClick(); });
  expect(json(r)).toContain('Original notes');
  await act(async () => { boxes(r)[1].props.onChange(); });
  expect(textOf(prepTab(r)!)).toBe('Prep 2/2');
});

test('readLiveSideTab accepts prep', () => {
  localStorage.setItem(LIVE_SIDE_TAB_KEY, 'prep');
  expect(readLiveSideTab()).toBe('prep');
});

test('live fetch runs once per link strings and clears when recording stops', async () => {
  const r = await render();
  tx.calendarLink = { ...tx.calendarLink! };
  await act(async () => { r.update(live()); });
  expect(invoke.mock.calls.filter(([command]) => command === 'prep_get')).toHaveLength(1);
  recording.isRecording = false;
  await act(async () => { r.update(live()); });
  expect(prepTab(r)).toBeUndefined();
});

test('no calendar link or no recording does not fetch prep', async () => {
  tx.calendarLink = null;
  await render();
  tx.calendarLink = { uid: fixture().eventUid, start: fixture().eventStart };
  recording.isRecording = false;
  await render();
  expect(invoke.mock.calls.filter(([command]) => command === 'prep_get')).toHaveLength(0);
});

test('late live result is ignored after link changes or unmount', async () => {
  const resolves: Array<(p: MeetingPrep | null) => void> = [];
  load = () => new Promise((resolve) => resolves.push(resolve));
  const r = await render();
  tx.calendarLink = { uid: 'new-event', start: 'new-start' };
  await act(async () => { r.update(live()); });
  await act(async () => { resolves[1](null); resolves[0](fixture()); });
  expect(prepTab(r)).toBeUndefined();
  tx.calendarLink = { uid: 'third-event', start: 'third-start' };
  await act(async () => { r.update(live()); });
  act(() => { r.unmount(); });
  await act(async () => { resolves[2](fixture()); });
});

test('late post-call result is ignored after meeting changes', async () => {
  const resolves: Array<(p: MeetingPrep | null) => void> = [];
  load = () => new Promise((resolve) => resolves.push(resolve));
  const r = await render(postCall());
  await act(async () => { r.update(postCall('meeting-2')); });
  await act(async () => { resolves[1](null); resolves[0](fixture()); });
  expect(prepTab(r)).toBeUndefined();
});

test('live and post-call load failures show toasts', async () => {
  load = async () => { throw new Error('read failed'); };
  await render();
  await render(postCall());
  expect(errorToast).toHaveBeenCalledTimes(2);
});

test('the notes child stays mounted when the prep arrives', async () => {
  let finish!: (p: MeetingPrep | null) => void;
  load = () => new Promise((resolve) => { finish = resolve; });
  let mounts = 0;
  let unmounts = 0;
  const Probe = () => {
    useEffect(() => { mounts++; return () => { unmounts++; }; }, []);
    return <div>Probe notes</div>;
  };
  const r = await render(<NotesWithPrep meetingId="meeting-1" notes={<Probe />} />);
  expect(mounts).toBe(1);
  await act(async () => { finish(fixture()); });
  expect(prepTab(r)).toBeDefined();
  expect(mounts).toBe(1);
  expect(unmounts).toBe(0);
});

test('two quick ticks resolving in reverse order keep the latest list', async () => {
  const pending: Array<{ args: Record<string, unknown>; resolve(p: MeetingPrep): void }> = [];
  tick = (args) => new Promise((resolve) => { pending.push({ args, resolve }); });
  const r = await render();
  act(() => { boxes(r)[1].props.onChange(); });
  act(() => { boxes(r)[0].props.onChange(); });
  expect(pending.map((p) => p.args)).toEqual([
    { prepId: 7, itemId: 'b', ticked: true },
    { prepId: 7, itemId: 'a', ticked: false },
  ]);
  const [a, b] = fixture().items;
  // The later untick's reply lands first: a off, b on.
  await act(async () => { pending[1].resolve({ ...fixture(), items: [{ ...a, ticked: false }, { ...b, ticked: true }] }); });
  expect(textOf(prepTab(r)!)).toBe('Prep 1/2');
  // The earlier tick's reply lands last with a stale list (a still on). It must not win.
  await act(async () => { pending[0].resolve({ ...fixture(), items: [{ ...a, ticked: true }, { ...b, ticked: true }] }); });
  expect(textOf(prepTab(r)!)).toBe('Prep 1/2');
  expect(boxes(r)[0].props.checked).toBe(false);
  expect(boxes(r)[1].props.checked).toBe(true);
});
