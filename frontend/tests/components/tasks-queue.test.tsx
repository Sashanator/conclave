import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { createRoot } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import type { TaskRow } from '../../src/lib/tasks';

const originals = { core: { ...(await import('@tauri-apps/api/core')) }, event: { ...(await import('@tauri-apps/api/event')) }, sonner: { ...(await import('sonner')) } };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('sonner', () => originals.sonner);
});

const mk = (id: string, over: Partial<TaskRow>): TaskRow => ({
  id, meetingId: 'm2', meetingTitle: 'M2', meetingDate: '2026-11-18 10:00:00+00:00', text: `Task ${id}`, owner: 'me', due: null, at: null,
  status: 'open', source: 'auto', createdAt: 't', review: 'open', removedBy: null, destId: null, dueDate: null, notionUrl: null, sentAt: null,
  sendError: null, sendAmbiguous: false, templateId: null, ...over,
});
const dest = (id: string, position: number) => ({
  id, name: `Dest ${id}`, colour: position + 1, position, databaseId: id, dataSourceId: null,
  mapping: { titleProp: 'Name', dueProp: null, meetingProp: null, fixed: [] }, lastStatus: 'connected', lastMessage: null,
});

let rows: TaskRow[] = [];
let failNext = false;
let sendReport: any = { sent: 1, failed: 0, ambiguous: 0 };
let sendGate: Promise<void> | null = null;
let sendError: Error | null = null;
let listGate: Promise<void> | null = null;
const calls: [string, any][] = [];
const invoke = mock(async (cmd: string, args?: any): Promise<unknown> => {
  calls.push([cmd, args]);
  if (cmd === 'tasks_queue_list') { if (listGate) await listGate; return structuredClone(rows); }
  if (cmd === 'notion_get_config') return { tokenLast4: 'abcd', destinations: [dest('d1', 0), dest('d2', 1)], categoryDefaults: [] };
  if (cmd === 'api_list_templates') return { templates: [] };
  if (cmd === 'open_external_url') return null;
  if (cmd === 'notion_send') {
    if (sendGate) await sendGate;
    if (sendError) throw sendError;
    return sendReport;
  }
  if (cmd === 'tasks_mark_sent') { const r = rows.find((x) => x.id === args.id)!; r.review = 'sent'; r.sendAmbiguous = false; return null; }
  if (failNext) { failNext = false; throw new Error('boom'); }
  const r = rows.find((x) => x.id === args.id)!;
  if (cmd === 'tasks_set_review') { r.review = args.review; r.removedBy = args.review === 'drop' ? 'user' : null; }
  if (cmd === 'tasks_set_dest') r.destId = args.destId;
  if (cmd === 'tasks_edit') r.text = args.text;
  if (cmd === 'tasks_set_due_date') r.dueDate = args.dueDate;
  return null;
});
const toast = Object.assign(mock(() => {}), { error: mock((_: string) => {}), dismiss: mock(() => {}) });
mock.module('@tauri-apps/api/core', () => ({ invoke }));
let tasksChanged: (() => void) | null = null;
mock.module('@tauri-apps/api/event', () => ({ listen: async (_n: string, h: () => void) => { tasksChanged = h; return () => { tasksChanged = null; }; } }));
mock.module('sonner', () => ({ toast }));
mock.module('next/link', () => ({ default: ({ href, children, ...p }: any) => <a href={href} {...p}>{children}</a> }));

const { TasksQueue } = await import('../../src/components/Tasks/TasksQueue');

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const writes = () => calls.filter(([c]) => c.startsWith('tasks_') && c !== 'tasks_queue_list');
const row = (id: string) => document.querySelector<HTMLElement>(`[data-row-id="${id}"]`)!;
const key = async (el: HTMLElement, k: string) => { await act(async () => { el.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true })); }); await flush(); };
const click = async (el: Element) => { await act(async () => { (el as HTMLElement).click(); }); await flush(); };
const setValue = async (el: HTMLInputElement, v: string) => {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(el, v);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  });
};
const text = () => document.body.textContent ?? '';

let root: ReturnType<typeof createRoot>;
async function mount() {
  if (root) await act(async () => { root.unmount(); }); // window listeners must not pile up across mounts
  const host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => { root.render(<TasksQueue />); });
  await flush();
}

beforeEach(async () => {
  document.body.innerHTML = '';
  calls.length = 0;
  failNext = false;
  sendReport = { sent: 1, failed: 0, ambiguous: 0 };
  sendGate = null;
  listGate = null;
  sendError = null;
  rows = [
    mk('a', {}), mk('b', {}),
    mk('c', { meetingId: 'm1', meetingTitle: 'M1', meetingDate: '2026-11-10 10:00:00+00:00' }),
    mk('z', { meetingId: 'm1', meetingTitle: 'M1', meetingDate: '2026-11-10 10:00:00+00:00', review: 'drop', removedBy: 'ai', owner: 'unassigned' }),
    mk('d', { meetingId: null, meetingTitle: 'Gone', meetingDate: '2026-11-01 10:00:00+00:00' }),
    mk('s', { review: 'sent', notionUrl: 'https://www.notion.so/p1', sentAt: '2026-11-19 09:30:00+00:00', destId: 'd1' }),
  ];
  await mount();
});

describe('tasks queue', () => {
  test('tabs and groups', () => {
    expect(document.querySelector('#tab-review')!.textContent).toContain('4');
    expect(Array.from(document.querySelectorAll('section')).map((s) => s.getAttribute('aria-label'))).toEqual(['M2', 'M1', 'Deleted meeting: Gone']);
    expect(text()).toContain("1 removed (AI thinks they aren't yours) · Show");
  });

  test('K keeps and advances; X folds and advances; X on the last row goes back', async () => {
    row('a').focus();
    await key(row('a'), 'K');
    expect(writes()[0]).toEqual(['tasks_set_review', { id: 'a', review: 'keep' }]);
    expect(document.activeElement).toBe(row('b'));
    await key(row('b'), 'X');
    expect(document.activeElement).toBe(row('c'));
    expect(row('b')).toBeNull();
    row('d').focus();
    await key(row('d'), 'x');
    expect(document.activeElement).toBe(row('c'));
  });

  test('clicking Keep on a kept row reopens it and focus stays', async () => {
    await key(row('a'), 'k');
    const btn = row('a').querySelector<HTMLElement>('button[aria-label="Keep: Task a"]')!;
    btn.focus();
    await click(btn);
    expect(writes().at(-1)).toEqual(['tasks_set_review', { id: 'a', review: 'open' }]);
    expect(row('a').contains(document.activeElement)).toBe(true);
  });

  test('1-9 pick a destination and focus stays; typing in the text input is ignored', async () => {
    row('a').focus();
    await key(row('a'), '2');
    expect(writes()[0]).toEqual(['tasks_set_dest', { id: 'a', destId: 'd2' }]);
    expect(document.activeElement).toBe(row('a'));
    await click(row('a').querySelector('button.block')!);
    const input = row('a').querySelector<HTMLInputElement>('input')!;
    const n = writes().length;
    await key(input, 'k');
    await key(input, '1');
    expect(writes().length).toBe(n);
  });

  test('Enter saves the edited text; Escape cancels and refocuses the row', async () => {
    await click(row('a').querySelector('button.block')!);
    await setValue(row('a').querySelector('input')!, 'New text');
    await key(row('a').querySelector('input')!, 'Enter');
    expect(writes()[0]).toEqual(['tasks_edit', { id: 'a', text: 'New text' }]);
    expect(document.activeElement).toBe(row('a'));
    await click(row('b').querySelector('button.block')!);
    await key(row('b').querySelector('input')!, 'Escape');
    expect(writes().length).toBe(1);
    expect(document.activeElement).toBe(row('b'));
  });

  test('due date saves on Enter or blur, only when changed, never while typing', async () => {
    await click(row('a').querySelector('button[aria-label^="Due date"]')!);
    const input = () => row('a').querySelector<HTMLInputElement>('input[type="date"]')!;
    await setValue(input(), '2026-11-26');
    expect(writes()).toEqual([]);
    await key(input(), 'Enter');
    expect(writes()).toEqual([['tasks_set_due_date', { id: 'a', dueDate: '2026-11-26' }]]);
    await click(row('a').querySelector('button[aria-label^="Due date"]')!);
    await act(async () => { input().dispatchEvent(new FocusEvent('focusout', { bubbles: true })); });
    await flush();
    expect(writes().length).toBe(1); // unchanged value: no save
    await click(row('a').querySelector('button[aria-label^="Due date"]')!);
    await setValue(input(), '');
    await act(async () => { input().dispatchEvent(new FocusEvent('focusout', { bubbles: true })); });
    await flush();
    expect(writes()[1]).toEqual(['tasks_set_due_date', { id: 'a', dueDate: null }]);
  });

  test('saving a due date on blur does not pull focus back from where the user went', async () => {
    await click(row('a').querySelector('button[aria-label^="Due date"]')!);
    const input = row('a').querySelector<HTMLInputElement>('input[type="date"]')!;
    input.focus();
    await setValue(input, '2026-11-26');
    const other = row('b').querySelector<HTMLElement>('button.block')!;
    await act(async () => { other.focus(); });
    await flush();
    expect(writes()).toEqual([['tasks_set_due_date', { id: 'a', dueDate: '2026-11-26' }]]);
    expect(document.activeElement).toBe(other);
    await click(other);
    expect(row('b').querySelector('input')).not.toBeNull();
    expect(document.activeElement).toBe(row('b').querySelector('input'));
  });

  test('restoring the last removed task keeps focus in the Removed tab', async () => {
    rows = [mk('r', { review: 'drop', removedBy: 'user' })];
    document.body.innerHTML = '';
    await mount();
    await click(document.querySelector('#tab-removed')!);
    await click(Array.from(document.querySelectorAll('button')).find((b) => b.textContent === 'Restore')!);
    expect(document.activeElement?.id).toBe('tab-removed');
  });

  test('Retry and Send again only on kept rows', async () => {
    rows = [mk('f', { review: 'open', sendError: 'boom', destId: 'd1' }), mk('g', { review: 'open', sendAmbiguous: true, destId: 'd1' })];
    document.body.innerHTML = '';
    await mount();
    const labels = Array.from(document.querySelectorAll('button')).map((b) => b.textContent);
    expect(labels).not.toContain('Retry');
    expect(labels).not.toContain('Send again');
    expect(labels).not.toContain('Mark as sent');
    expect(text()).toContain('Not sent: boom');
  });

  test('re-reads on tasks-changed', async () => {
    const n = () => calls.filter(([c]) => c === 'tasks_queue_list').length;
    const before = n();
    rows.push(mk('n', {}));
    await act(async () => { tasksChanged!(); });
    await flush();
    expect(n()).toBe(before + 1);
    expect(row('n')).not.toBeNull();
  });

  test('arrow keys move between tabs', async () => {
    await key(document.querySelector<HTMLElement>('#tab-review')!, 'ArrowRight');
    expect(document.activeElement?.id).toBe('tab-sent');
    expect(document.querySelector('#tab-sent')!.getAttribute('aria-selected')).toBe('true');
    await key(document.querySelector<HTMLElement>('#tab-sent')!, 'ArrowLeft');
    expect(document.activeElement?.id).toBe('tab-review');
  });

  test('Keep all mine keeps each open row of the group and focuses its first row', async () => {
    await click(document.querySelector('section[aria-label="M2"] button')!.parentElement!.querySelector('button:last-child')!);
    expect(writes().map(([c, a]) => [c, a.id, a.review])).toEqual([['tasks_set_review', 'a', 'keep'], ['tasks_set_review', 'b', 'keep']]);
    expect(document.activeElement).toBe(row('a'));
  });

  test('a failed change toasts and focus is not lost to the page', async () => {
    row('a').focus();
    failNext = true;
    await key(row('a'), 'K');
    expect(toast.error).toHaveBeenCalledWith('Could not save that change.');
    expect(document.activeElement).not.toBe(document.body);
  });

  test('Removed tab: Restore; Sent tab: Open in Notion', async () => {
    await click(document.querySelector('#tab-removed')!);
    expect(text()).toContain('M1 · Unassigned · removed by AI');
    await click(Array.from(document.querySelectorAll('button')).find((b) => b.textContent === 'Restore')!);
    expect(writes()[0]).toEqual(['tasks_set_review', { id: 'z', review: 'open' }]);
    expect(document.activeElement).not.toBe(document.body);
    await click(document.querySelector('#tab-sent')!);
    await click(Array.from(document.querySelectorAll('button')).find((b) => b.textContent === 'Open in Notion ↗')!);
    expect(calls.at(-1)).toEqual(['open_external_url', { url: 'https://www.notion.so/p1' }]);
  });

  test('empty queue reads All reviewed', async () => {
    document.body.innerHTML = '';
    rows = [];
    await mount();
    expect(text()).toContain('All reviewed');
  });
});

describe('sending', () => {
  const sends = () => calls.filter(([c]) => c === 'notion_send');
  const sendBtn = () => Array.from(document.querySelectorAll('button')).find((b) => /^(Send \d+ to Notion|Sending…|Nothing to send)$/.test(b.textContent!))!;
  const status = () => document.querySelector('[role="status"]')!.textContent!;
  const ctrlEnter = async (el: Element) => { await act(async () => { el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true, cancelable: true })); }); await flush(); };
  const setup = async (extra: TaskRow[] = []) => {
    rows = [
      mk('a', { review: 'keep', destId: 'd1' }), mk('b', { review: 'keep', destId: 'd1' }),
      ...extra,
    ];
    document.body.innerHTML = '';
    await mount();
  };

  test('bar counts', async () => {
    rows = [
      mk('a', { review: 'keep', destId: 'd1' }), mk('b', { review: 'keep' }), mk('c', { review: 'keep', destId: 'd2' }), mk('e', {}),
    ];
    const orig = invoke.getMockImplementation()!;
    invoke.mockImplementation(async (cmd: string, args?: any) => cmd === 'notion_get_config'
      ? { tokenLast4: 'abcd', destinations: [dest('d1', 0), { ...dest('d2', 1), name: 'Personal tasks', lastStatus: 'not_shared' }], categoryDefaults: [] }
      : orig(cmd, args));
    document.body.innerHTML = '';
    await mount();
    invoke.mockImplementation(orig);
    expect(text()).toContain('3 kept');
    expect(text()).toContain('1 need a database');
    expect(text()).toContain('1 waiting for Personal tasks');
    expect(text()).toContain('1 not decided');
  });

  test('send uses explicit ids and reports', async () => {
    await setup();
    sendReport = { sent: 2, failed: 0, ambiguous: 0 };
    await click(sendBtn());
    expect(sends()).toEqual([['notion_send', { taskIds: ['a', 'b'], resendAmbiguous: false }]]);
    expect(status()).toContain('Sent 2 to Notion');
    expect(status()).toContain('View');
  });

  test('in flight: disabled, no second send by click or Ctrl+Enter', async () => {
    await setup();
    let release!: () => void;
    sendGate = new Promise((r) => { release = r; });
    await click(sendBtn());
    expect(sendBtn().textContent).toBe('Sending…');
    expect((sendBtn() as HTMLButtonElement).disabled).toBe(true);
    await click(sendBtn());
    await ctrlEnter(row('a'));
    release();
    await flush();
    expect(sends().length).toBe(1);
  });

  test('stays blocked until the post-send reload finishes', async () => {
    await setup();
    let release!: () => void;
    await click(sendBtn());
    sendReport = { sent: 2, failed: 0, ambiguous: 0 };
    listGate = new Promise((r) => { release = r; });
    await click(sendBtn()); // second send: its reload now hangs
    await click(sendBtn());
    await ctrlEnter(row('a'));
    expect(sends().length).toBe(2);
    release();
    await flush();
  });

  test('same-tick double click sends once', async () => {
    await setup();
    sendGate = new Promise(() => {});
    await act(async () => { sendBtn().click(); sendBtn().click(); });
    expect(sends().length).toBe(1);
  });

  test('nothing to send is disabled; Ctrl+Enter from a row sends, from the text input does not', async () => {
    rows = [mk('a', {})];
    document.body.innerHTML = '';
    await mount();
    expect(sendBtn().textContent).toBe('Nothing to send');
    expect((sendBtn() as HTMLButtonElement).disabled).toBe(true);
    await setup();
    row('a').querySelector<HTMLElement>('button.block')!.click();
    await flush();
    await ctrlEnter(document.querySelector('input[aria-label^="Edit task"]')!);
    expect(sends().length).toBe(0);
    await ctrlEnter(row('b'));
    expect(sends().length).toBe(1);
  });

  test('failed report shows the line; Retry sends that one row', async () => {
    await setup();
    sendReport = { sent: 1, failed: 1, ambiguous: 0 };
    rows[1].sendError = 'Notion said no';
    await click(sendBtn());
    expect(status()).toContain('1 not sent, see To review');
    expect(row('b').textContent).toContain('Not sent: Notion said no');
    const retry = Array.from(row('b').querySelectorAll('button')).find((b) => b.textContent === 'Retry')!;
    await click(retry);
    expect(sends().at(-1)).toEqual(['notion_send', { taskIds: ['b'], resendAmbiguous: false }]);
  });

  test('a rejected send shows the text and marks nothing sent', async () => {
    await setup();
    sendError = new Error('A send is already running');
    await click(sendBtn());
    expect(status()).toContain('A send is already running');
    expect(text()).not.toContain('Sent 1');
    expect(document.querySelectorAll('[data-row-id]').length).toBe(2);
  });

  test('ambiguous row: open database, mark as sent, send again; never in the bulk send', async () => {
    await setup([mk('m', { review: 'keep', destId: 'd1', sendAmbiguous: true })]);
    expect(row('m').textContent).toContain('May already be in Notion. Check the database before sending again.');
    const btn = (label: string) => Array.from(row('m').querySelectorAll('button')).find((b) => b.textContent === label)!;
    await click(sendBtn());
    expect(sends()[0][1]).toEqual({ taskIds: ['a', 'b'], resendAmbiguous: false });
    await click(btn('Open Notion database'));
    expect(calls).toContainEqual(['open_external_url', { url: 'https://www.notion.so/d1' }]);
    await click(btn('Send again'));
    expect(sends().at(-1)).toEqual(['notion_send', { taskIds: ['m'], resendAmbiguous: true }]);
    await click(btn('Mark as sent'));
    expect(calls).toContainEqual(['tasks_mark_sent', { id: 'm' }]);
  });

  test('after a send, focus stays on the row the user was on', async () => {
    await setup([mk('e', {}), mk('f', {})]);
    row('f').focus();
    await ctrlEnter(row('f'));
    expect(sends().length).toBe(1);
    expect(document.activeElement).toBe(row('f'));
  });

  test('bar only on To review; focus lands on a row after a send', async () => {
    await setup();
    await click(sendBtn());
    expect(document.activeElement === document.querySelector('#tab-review') || !!document.activeElement?.closest('[data-row-id]')).toBe(true);
    await click(document.querySelector('#tab-sent')!);
    expect(sendBtn()).toBeUndefined();
    await click(document.querySelector('#tab-removed')!);
    expect(sendBtn()).toBeUndefined();
  });
});
