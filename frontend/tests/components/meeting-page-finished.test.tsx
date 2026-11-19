import { afterAll, beforeEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { TaskRow } from '../../src/lib/tasks';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  sonner: { ...await import('sonner') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
});

const task = (over: Partial<TaskRow>): TaskRow => ({
  id: 't1', meetingId: 'm1', meetingTitle: 'M', meetingDate: null, text: 'Send the deck', owner: 'me', due: null,
  at: null, status: 'open', source: 'auto', createdAt: '2026-11-16T10:00:00Z', ...over,
});
let rows: TaskRow[] = [];
let failing = false;
let failCommand: string | null = null; // fails only this command
let failGate: Promise<void> | null = null; // and only after the gate opens
const calls: [string, Record<string, unknown> | undefined][] = [];
const errors: string[] = [];
mock.module('@tauri-apps/api/core', () => ({
  invoke: async (command: string, args?: Record<string, unknown>) => {
    calls.push([command, args]);
    if (command === 'meeting_cost') return { total_usd: 0.012, tokens: 18400, by_purpose: [{ purpose: 'summary', tokens: 18400, cost_usd: 0.012 }] };
    if (command === 'tasks_list') return rows.filter((r) => r.status === args?.status);
    if (failCommand === command && (args?.id === 't2' || args?.id === 'u1')) { await failGate; throw new Error('boom'); }
    if (failing) throw new Error('boom');
    return null;
  },
}));
mock.module('sonner', () => ({ toast: { error: (m: string) => errors.push(m), success() {}, warning() {}, info() {} } }));

const { MeetingTasksPanel } = await import('../../src/components/MeetingPage/MeetingTasksPanel');
const { openingTab } = await import('../../src/components/MeetingPage/FinishedTabs');
const { lineAtTime, parseMeetingTarget } = await import('../../src/lib/meeting-target');

beforeEach(() => { calls.length = 0; errors.length = 0; failing = false; failCommand = null; failGate = null; });

async function mount(props: { onCountChange?: (n: number) => void } = {}) {
  let r!: ReactTestRenderer;
  await act(async () => { r = create(<MeetingTasksPanel meetingId="m1" {...props} />); });
  return r;
}
const plain = (n: ReactTestInstance | string): string => (typeof n === 'string' ? n : n.children.map(plain).join(''));
const row = (r: ReactTestRenderer, text: string): ReactTestInstance =>
  r.root.findAll((n) => n.type === 'li' && plain(n).includes(text))[0];
const press = async (n: ReactTestInstance, key: string) => {
  await act(async () => { n.props.onKeyDown({ key, preventDefault() {} }); });
};
const buttons = (n: ReactTestInstance) => n.findAll((x) => x.type === 'button');
const text = (r: ReactTestRenderer) => plain(r.root);

test('opens on Summary; a time or a tab link opens its tab', () => {
  expect(openingTab(null, null)).toBe('summary');
  const segs = [0, 12, 24].map((timestamp, i) => ({ id: `t-${i}`, timestamp }));
  const focus = lineAtTime(segs, 24, false);
  expect(focus).toBe('t-2');
  expect(openingTab(parseMeetingTarget(new URLSearchParams('id=m1&t=24'))?.tab, focus)).toBe('transcript');
  expect(openingTab(parseMeetingTarget(new URLSearchParams('id=m1&tab=prep'))?.tab, null)).toBe('prep');
});

test('an unassigned task sits under Removed with Keep; k claims it', async () => {
  rows = [task({ id: 'u1', text: 'Run QA', owner: 'unassigned' })];
  const r = await mount();
  expect(text(r)).toContain('Removed (1)');
  expect(text(r)).toContain('removed by AI');
  await act(async () => { buttons(row(r, 'Run QA'))[0].props.onClick(); });
  expect(calls).toContainEqual(['tasks_claim', { id: 'u1' }]);
});

test('x dismisses a kept task; a failure toasts and rolls back', async () => {
  rows = [task({})];
  const r = await mount();
  await press(row(r, 'Send the deck'), 'x');
  expect(calls).toContainEqual(['tasks_set_status', { id: 't1', status: 'dismissed' }]);
  expect(text(r)).toContain('Removed (1)');

  failing = true;
  rows = [task({ id: 't2', text: 'Other' })];
  const r2 = await mount();
  await press(row(r2, 'Other'), 'x');
  expect(errors.length).toBe(1);
  expect(text(r2)).not.toContain('Removed (');
});

test('a failed edit rolls back only its own task', async () => {
  rows = [task({}), task({ id: 't2', text: 'Other' })];
  const r = await mount();
  let open!: () => void;
  failGate = new Promise<void>((res) => { open = res; });
  failCommand = 'tasks_set_status';
  row(r, 'Other').props.onKeyDown({ key: 'x', preventDefault() {} }); // t2 is still saving when t1 is removed (no act: acts must not overlap)
  failCommand = null;
  await press(row(r, 'Send the deck'), 'x');
  failCommand = 'tasks_set_status'; open();
  await act(async () => { await new Promise((r) => setTimeout(r, 10)); });
  expect(errors.length).toBe(1);
  expect(text(r)).toContain('Removed (1)'); // t1 stays removed
  expect(text(r)).toContain('Other');
  expect(row(r, 'Other').findAll((n) => n.type === 'button' && plain(n) === 'Remove X').length).toBe(1);
});

test('a restore whose status step fails keeps the saved claim', async () => {
  rows = [{ ...task({ id: 'u1', text: 'Run QA', owner: 'unassigned' }), status: 'dismissed' }];
  failCommand = 'tasks_set_status';
  const r = await mount();
  await press(row(r, 'Run QA'), 'k');
  expect(errors.length).toBe(1);
  expect(text(r)).toContain('removed');
  expect(text(r)).not.toContain('removed by AI');
});

test('Done reviewing marks the meeting reviewed, then reads Reviewed', async () => {
  rows = [];
  const r = await mount();
  const btn = r.root.findAll((n) => n.type === 'button' && plain(n).includes('Done reviewing'))[0];
  await act(async () => { btn.props.onClick(); });
  expect(calls).toContainEqual(['api_set_meeting_reviewed', { meetingId: 'm1', reviewed: true }]);
  expect(text(r)).toContain('Reviewed ✓');
});

test('every task shows Keep and Remove with K and X hints', async () => {
  rows = [task({ id: 't1', text: 'One' }), task({ id: 't2', text: 'Two' })];
  const r = await mount();
  for (const t of ['One', 'Two']) expect(buttons(row(r, t)).map(plain)).toEqual(['Keep K', 'Remove X']);
  expect(buttons(row(r, 'One'))[0].props['aria-pressed']).toBe(true);
});

test('reports the kept count', async () => {
  rows = [task({ id: 't1', text: 'One' }), task({ id: 't2', text: 'Two' })];
  const counts: number[] = [];
  await mount({ onCountChange: (n) => counts.push(n) });
  expect(counts[counts.length - 1]).toBe(2);
});

test('shows the LLM cost line', async () => {
  rows = [];
  const r = await mount();
  expect(text(r)).toContain('LLM cost: $0.01');
});
