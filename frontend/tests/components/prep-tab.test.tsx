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
  calendarLink: { uid: fixture().eventUid!, start: fixture().eventStart! } as { uid: string; start: string } | null,
  notes: [], addNote: mock(() => {}), updateNote: mock(() => {}), deleteNote: mock(() => {}),
};
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals.recording, useRecordingState: () => recording }));
mock.module('../../src/contexts/TranscriptContext', () => ({ ...originals.transcripts, useTranscripts: () => tx }));
mock.module('../../src/app/_components/TranscriptPanel', () => ({ TranscriptPanel: () => null }));
mock.module('../../src/components/Copilot/CopilotPane', () => ({ CopilotPane: () => <div>Copilot body</div> }));

const { LiveTranscriptAndNotes } = await import('../../src/app/_components/LiveTranscriptAndNotes');
const live = () => <LiveTranscriptAndNotes isProcessingStop={false} isStopping={false} showModal={() => {}} />;
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
  tx.calendarLink = { uid: fixture().eventUid!, start: fixture().eventStart! };
  invoke.mockClear();
  errorToast.mockClear();
});
afterEach(() => { act(() => { renderers.splice(0).forEach((r) => r.unmount()); }); });

test('no prep: no points and no tab', async () => {
  result = null;
  const r = await render();
  expect(invoke).toHaveBeenCalledWith('prep_get', { eventUid: fixture().eventUid!, eventStart: fixture().eventStart! });
  expect(prepTab(r)).toBeUndefined();
  expect(boxes(r)).toHaveLength(0);
});

test('prep points sit under My notes with kind tags, and draft label', async () => {
  const r = await render();
  expect(prepTab(r)).toBeUndefined();
  expect(json(r)).toContain('draft');
  expect(json(r)).toContain('clarification');
  expect(r.root.findAll((n) => n.type === 'a')).toHaveLength(0);
  expect(r.root.findByProps({ 'aria-label': 'New note' })).toBeDefined();
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
  await act(async () => { boxes(r)[1].props.onChange(); });
  expect(invoke).toHaveBeenCalledWith('prep_tick', { prepId: 7, itemId: 'b', ticked: true });
  expect(boxes(r)[1].props.checked).toBe(false);
  await act(async () => { resolve({ ...fixture(), items: fixture().items.map((i) => ({ ...i, ticked: true })) }); });
  expect(boxes(r)[1].props.checked).toBe(true);
});

test('tick failure reverts and shows a toast', async () => {
  tick = async () => { throw new Error('write failed'); };
  const r = await render();
  await act(async () => { boxes(r)[1].props.onChange(); });
  expect(boxes(r)[1].props.checked).toBe(false);
  expect(errorToast).toHaveBeenCalledTimes(1);
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
  tx.calendarLink = { uid: fixture().eventUid!, start: fixture().eventStart! };
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

test('a live load failure shows a toast', async () => {
  load = async () => { throw new Error('read failed'); };
  await render();
  expect(errorToast).toHaveBeenCalledTimes(1);
});

test('two quick ticks on different boxes serialize and both stay ticked', async () => {
  result = { ...fixture(), items: fixture().items.map(i => ({ ...i, ticked: false })) };
  const pending: Array<{ args: Record<string, unknown>; resolve(p: MeetingPrep): void }> = [];
  tick = args => new Promise(resolve => { pending.push({ args, resolve }); });
  const r = await render();
  await act(async () => { boxes(r)[0].props.onChange(); boxes(r)[1].props.onChange(); });
  expect(pending.map(p => p.args)).toEqual([{ prepId: 7, itemId: 'a', ticked: true }]);
  const [a, b] = result.items;
  await act(async () => { pending[0].resolve({ ...fixture(), items: [{ ...a, ticked: true }, b] }); });
  expect(pending.map(p => p.args)).toEqual([
    { prepId: 7, itemId: 'a', ticked: true },
    { prepId: 7, itemId: 'b', ticked: true },
  ]);
  await act(async () => { pending[1].resolve({ ...fixture(), items: [{ ...a, ticked: true }, { ...b, ticked: true }] }); });
  expect(boxes(r).map(b => b.props.checked)).toEqual([true, true]);
});

test('double-clicking a box sends true then false', async () => {
  let applied = fixture();
  tick = async args => {
    applied = { ...applied, items: applied.items.map(i => i.id === args.itemId ? { ...i, ticked: args.ticked as boolean } : i) };
    return applied;
  };
  const r = await render();
  await act(async () => { boxes(r)[1].props.onChange(); boxes(r)[1].props.onChange(); });
  expect(invoke.mock.calls.filter(([c]) => c === 'prep_tick').map(([, args]) => args)).toEqual([
    { prepId: 7, itemId: 'b', ticked: true },
    { prepId: 7, itemId: 'b', ticked: false },
  ]);
  expect(boxes(r)[1].props.checked).toBe(false);
});

test('a rejected tick leaves the queue usable for the next click', async () => {
  let attempts = 0;
  tick = async () => {
    if (++attempts === 1) throw new Error('write failed');
    return { ...fixture(), items: fixture().items.map(i => ({ ...i, ticked: true })) };
  };
  const r = await render();
  await act(async () => { boxes(r)[1].props.onChange(); boxes(r)[1].props.onChange(); });
  expect(invoke.mock.calls.filter(([c]) => c === 'prep_tick').map(([, args]) => args)).toEqual([
    { prepId: 7, itemId: 'b', ticked: true },
    { prepId: 7, itemId: 'b', ticked: true },
  ]);
  expect(errorToast).toHaveBeenCalledTimes(1);
  expect(boxes(r)[1].props.checked).toBe(true);
});
