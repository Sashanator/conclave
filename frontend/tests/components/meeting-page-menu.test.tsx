import { afterAll, beforeEach, expect, mock, test } from 'bun:test';
import { createRoot } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import type { MeetingRow } from '../../src/lib/meetings';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  sonner: { ...await import('sonner') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
});

const calls: Array<[string, unknown]> = [];
let failWith: string | null = null;
let deleteResult: unknown = null;
let deleteGate: Promise<void> | null = null;
const invoke = mock(async (command: string, args?: unknown) => {
  calls.push([command, args]);
  if (command === 'api_list_templates') return { templates: [{ id: 'one', name: '1:1', emoji: '🗣' }], errors: [] };
  if (command.startsWith('api_delete_') && failWith) throw new Error(failWith);
  if (command.startsWith('api_delete_')) { await deleteGate; return deleteResult; }
  return null;
});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
const toasts: Array<[string, string, unknown]> = [];
const t = (kind: string) => (msg: string, opts?: unknown) => { toasts.push([kind, msg, opts]); };
mock.module('sonner', () => ({ toast: { error: t('error'), success: t('success'), warning: t('warning'), info: t('info') } }));

const { MeetingHeader } = await import('../../src/components/MeetingPage/MeetingHeader');

const row: MeetingRow = {
  id: 'm1', title: 'Weekly', titleSource: 'user', templateId: null, emoji: null, reviewed: false, gist: null,
  startedAt: '2026-11-16 10:00:00+00:00', endedAt: '2026-11-16 11:00:00+00:00', createdAt: '2026-11-16 10:00:00+00:00', calendarUid: null,
  calendarStart: null, calendarEnd: null, calendarCancelledAt: null, hasTranscript: true, hasSummary: true, prepPoints: 0, hasPrep: false,
};
beforeEach(() => { calls.length = 0; toasts.length = 0; failWith = null; deleteResult = null; deleteGate = null; });

type Props = Partial<Parameters<typeof MeetingHeader>[0]>;
async function mount(props: Props = {}) {
  const host = document.createElement('div');
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => { root.render(<MeetingHeader row={row} onChange={() => {}} {...props} />); });
  const trigger = host.querySelector('button[aria-label="Meeting actions"]') as HTMLElement;
  const items = () => Array.from(document.querySelectorAll('[role="menuitem"]')) as HTMLElement[];
  const openMenu = async () => { await act(async () => { trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })); }); };
  const click = async (el: Element) => { await act(async () => { (el as HTMLElement).click(); }); };
  const button = (label: string) => Array.from(document.querySelectorAll('button')).find((b) => b.textContent === label)!;
  return { host, root, items, openMenu, click, button, close: async () => { await act(async () => { root.unmount(); }); host.remove(); document.body.innerHTML = ''; } };
}
const deletes = () => calls.filter(([c]) => c.startsWith('api_delete_'));

test('Delete meeting asks first, then deletes and leaves for the list', async () => {
  let gone = 0;
  const m = await mount({ finished: true, onMeetingDeleted: () => { gone++; } });
  await m.openMenu();
  await m.click(m.items().find((i) => i.textContent === 'Delete meeting…')!);
  expect(deletes()).toEqual([]);
  expect(document.body.textContent).toContain('This removes:');
  await m.click(m.button('Delete meeting'));
  expect(deletes()).toEqual([['api_delete_meeting', { meetingId: 'm1' }]]);
  expect(gone).toBe(1);
  await m.close();
});

test('Cancel makes no call', async () => {
  const m = await mount({ finished: true });
  await m.openMenu();
  await m.click(m.items().find((i) => i.textContent === 'Delete meeting…')!);
  await m.click(m.button('Cancel'));
  expect(deletes()).toEqual([]);
  await m.close();
});

test('Delete recording says what stays, deletes only the recording and reloads', async () => {
  let reloaded = 0;
  const m = await mount({ finished: true, onRecordingDeleted: () => { reloaded++; } });
  await m.openMenu();
  await m.click(m.items().find((i) => i.textContent === 'Delete recording…')!);
  expect(document.body.textContent).toContain('Title, category, prep and notes stay.');
  expect(deletes()).toEqual([]);
  await m.click(m.button('Delete recording'));
  expect(deletes()).toEqual([['api_delete_recording', { meetingId: 'm1' }]]);
  expect(reloaded).toBe(1);
  await m.close();
});

test('a New meeting offers no Delete recording', async () => {
  const m = await mount({ finished: false });
  await m.openMenu();
  const labels = m.items().map((i) => i.textContent);
  expect(labels).toContain('Delete meeting…');
  expect(labels).not.toContain('Delete recording…');
  await m.close();
});

test('both deletes are disabled while the meeting is live', async () => {
  const m = await mount({ finished: true, live: true });
  await m.openMenu();
  for (const i of m.items()) expect(i.getAttribute('aria-disabled')).toBe('true');
  await m.close();
});

test('a failed delete toasts the error and the page stays', async () => {
  failWith = 'This meeting is being recorded';
  let gone = 0;
  const m = await mount({ finished: true, onMeetingDeleted: () => { gone++; } });
  await m.openMenu();
  await m.click(m.items().find((i) => i.textContent === 'Delete meeting…')!);
  await m.click(m.button('Delete meeting'));
  expect(gone).toBe(0);
  expect(toasts.some(([k, , o]) => k === 'error' && JSON.stringify(o).includes('being recorded'))).toBe(true);
  await m.close();
});

test('a folder warning is shown and the delete still completes', async () => {
  deleteResult = { folder_warning: 'folder locked' };
  let gone = 0;
  const m = await mount({ finished: true, onMeetingDeleted: () => { gone++; } });
  await m.openMenu();
  await m.click(m.items().find((i) => i.textContent === 'Delete meeting…')!);
  await m.click(m.button('Delete meeting'));
  expect(gone).toBe(1);
  expect(toasts.some(([k, , o]) => k === 'warning' && JSON.stringify(o).includes('folder locked'))).toBe(true);
  await m.close();
});

test('category change on a finished meeting only saves it: no summary run, no toast to regenerate', async () => {
  let changed = null as MeetingRow | null;
  const m = await mount({ finished: true, onChange: (r) => { changed = r; } });
  const select = m.host.querySelector('select[aria-label="Category"]') as HTMLSelectElement;
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value')!.set!;
    setter.call(select, 'one');
    select.dispatchEvent(new Event('change', { bubbles: true }));
  });
  expect(calls.find(([c]) => c === 'api_set_meeting_template')?.[1]).toEqual({ meetingId: 'm1', templateId: 'one' });
  expect(calls.some(([c]) => c === 'api_process_transcript')).toBe(false);
  expect(changed!.templateId).toBe('one');
  expect(toasts.length).toBe(0);
  await m.close();
});

test('the menu stays disabled until the delete request settles', async () => {
  let release!: () => void;
  deleteGate = new Promise<void>((r) => { release = r; });
  const m = await mount({ finished: true });
  await m.openMenu();
  await m.click(m.items().find((i) => i.textContent === 'Delete meeting…')!);
  await m.click(m.button('Delete meeting'));
  await m.openMenu();
  for (const i of m.items()) expect(i.getAttribute('aria-disabled')).toBe('true');
  await act(async () => { release(); });
  await m.openMenu();
  for (const i of m.items()) expect(i.getAttribute('aria-disabled')).toBeNull();
  await m.close();
});

test('header actions render before the menu', async () => {
  const m = await mount({ actions: <button type="button">X</button> });
  const labels = Array.from(m.host.querySelectorAll('button')).map((b) => b.getAttribute('aria-label') ?? b.textContent);
  expect(labels.indexOf('X')).toBeGreaterThanOrEqual(0);
  expect(labels.indexOf('X')).toBeLessThan(labels.indexOf('Meeting actions'));
  await m.close();
});
