import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingPrep, PrepOverview } from '../../src/lib/prep';
import { SUGGEST_MESSAGE } from '../../src/lib/prep';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  navigation: { ...await import('next/navigation') },
  sonner: { ...await import('sonner') },
};
afterAll(() => {
  renderer?.unmount();
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('next/navigation', () => originals.navigation);
  mock.module('sonner', () => originals.sonner);
});
const initial: MeetingPrep = {
  id: 1, eventUid: 'e1', eventStart: '2026-11-01T10:00:00Z', eventTitle: 'Planning',
  templateId: null, material: '', chat: [],
  items: [{ id: 'i1', text: 'Original question', kind: 'question', ticked: false, byUser: false }],
  approvedAt: null, createdAt: '2026-10-29T10:00:00Z', updatedAt: '2026-10-29T10:00:00Z',
};
let stored: MeetingPrep;
let overview: PrepOverview;
let limit = 100;
let failSave = false;
let failSend = false;
let missingPrep = false;
let pauseSave: Promise<void> | null = null;
let query = '';
const toastError = mock(() => {});
const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  switch (command) {
    case 'prep_overview': return structuredClone(overview);
    case 'prep_material_limit': return limit;
    case 'api_list_templates': return { templates: [{ id: 't1', name: 'Review' }], errors: [] };
    case 'prep_get': return missingPrep ? null : structuredClone(stored);
    case 'prep_save':
      if (pauseSave) await pauseSave;
      if (failSave) throw new Error('Write failed');
      stored = { ...stored, ...args } as MeetingPrep;
      return structuredClone(stored);
    case 'prep_send':
      if (failSend) throw new Error('Send failed');
      stored.chat = [{ role: 'assistant', text: '<b>Reply</b>', at: 'now', listNotUpdated: true }];
      return { prep: structuredClone(stored), listUpdated: false };
    case 'prep_approve': return { ...stored, approvedAt: stored.updatedAt };
    case 'prep_delete': return null;
    default: throw new Error(`Unexpected command: ${command}`);
  }
});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('next/navigation', () => ({ ...originals.navigation, useSearchParams: () => new URLSearchParams(query) }));
mock.module('sonner', () => ({ ...originals.sonner, toast: { ...originals.sonner.toast, error: toastError } }));
const { default: PrepPage } = await import('../../src/app/prep/page');
let renderer: ReactTestRenderer | null = null;
function text(node: ReactTestInstance): string {
  return node.children.map(c => typeof c === 'string' ? c : text(c)).join('');
}
function button(name: string) {
  return renderer!.root.findAll(n => n.type === 'button' && text(n).trim() === name)[0];
}
function field(label: string) { return renderer!.root.find(n => n.props['aria-label'] === label && typeof n.type === 'string'); }
function calls(command: string) { return invoke.mock.calls.filter(c => c[0] === command); }
async function flush() { await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); }); }
async function mount(selected = true) {
  query = selected ? 'event=e1&start=2026-11-01T10%3A00%3A00Z' : '';
  await act(async () => { renderer = create(<PrepPage />); });
  await flush();
}
async function change(label: string, value: string) {
  await act(async () => { field(label).props.onChange({ target: { value } }); });
  await flush();
}
async function blur(label: string) {
  await act(async () => { await field(label).props.onBlur(); });
  await flush();
}
async function keyDown(label: string, key: string) {
  await act(async () => { field(label).props.onKeyDown({ key, preventDefault: () => {} }); });
  await flush();
}
async function click(name: string) { await act(async () => { await button(name).props.onClick(); }); await flush(); }
beforeEach(() => {
  renderer?.unmount(); renderer = null;
  stored = structuredClone(initial); limit = 100; failSave = false; failSend = false; missingPrep = false; pauseSave = null;
  overview = { connected: true, upcoming: [{ uid: 'e1', title: 'Planning', start: initial.eventStart, end: initial.eventStart, templateId: null, templateName: null, prep: { id: 1, approved: false } }], other: [] };
  invoke.mockClear(); toastError.mockClear();
});
describe('Prep page', () => {
  test('not connected shows the settings link', async () => {
    overview.connected = false;
    await mount(false);
    expect(text(renderer!.root)).toContain('Calendar is not connected.');
    expect(renderer!.root.findByType('a').props.href).toBe('/settings?tab=knowledge');
  });
  test('lists events by day with General fallback and orphans', async () => {
    overview.upcoming.push({ ...overview.upcoming[0], uid: 'e2', title: 'Review meeting', start: '2026-11-02T10:00:00Z', templateName: 'Review', prep: { id: 2, approved: true } });
    overview.other.push({ id: 3, eventUid: 'gone', eventStart: initial.eventStart, eventTitle: 'Old event', approved: false, reason: 'not_in_calendar' });
    await mount(false);
    const content = text(renderer!.root);
    for (const expected of ['2026-11-01', '2026-11-02', 'General', 'Review', 'Draft', 'Approved', 'Past and orphaned preps', 'event no longer in calendar']) expect(content).toContain(expected);
    expect(calls('prep_save')).toHaveLength(0);
  });
  test('suggest sends the default message once material exists', async () => {
    await mount();
    expect(button('Suggest')).toBeUndefined();
    expect(calls('prep_send')).toHaveLength(0);
    expect(calls('prep_save')).toHaveLength(0);
    expect(calls('prep_get')[0][1]).toEqual({ eventUid: 'e1', eventStart: initial.eventStart });
    await change('Material', 'Notes');
    await click('Suggest');
    expect(calls('prep_send')).toEqual([['prep_send', { prepId: 1, message: SUGGEST_MESSAGE }]]);
    expect(calls('prep_save')[0][1]?.material).toBe('Notes');
  });
  test('suggest keeps a typed draft', async () => {
    await mount();
    await change('Material', 'Notes');
    await change('Message', 'Half written');
    await click('Suggest');
    expect(field('Message').props.value).toBe('Half written');
    expect(text(renderer!.root)).toContain('<b>Reply</b>');
  });
  test('reply without list shows list not updated', async () => {
    stored.material = 'Notes';
    await mount(); await click('Suggest');
    expect(text(renderer!.root)).toContain('list not updated');
    expect(text(renderer!.root)).toContain('<b>Reply</b>');
    expect(renderer!.root.findAllByType('b')).toHaveLength(0);
  });
  test('send error keeps the message and offers Retry', async () => {
    failSend = true;
    await mount(); await change('Message', 'My question'); await click('Send');
    expect(field('Message').props.value).toBe('My question');
    expect(text(renderer!.root)).toContain('Send failed');
    expect(button('Retry')).toBeDefined();
    failSend = false; await click('Retry');
    expect(field('Message').props.value).toBe('');
    expect(calls('prep_send')).toHaveLength(2);
  });
  test('material over the limit is refused with the count', async () => {
    limit = 10;
    await mount(); await change('Material', '12345678901');
    await blur('Material');
    expect(text(renderer!.root)).toContain('11 / 10 characters');
    expect(calls('prep_save')).toHaveLength(0);
  });
  test('non-utf8 txt is refused', async () => {
    stored.material = 'Keep this';
    await mount();
    const file = new File([new Uint8Array([0xC0, 0xE1])], 'bad.txt');
    await act(async () => { await field('Add .txt file').props.onChange({ target: { files: [file], value: 'bad.txt' } }); });
    expect(toastError).toHaveBeenCalledWith('The file is not UTF-8 text');
    expect(field('Material').props.value).toBe('Keep this');
    expect(calls('prep_save')).toHaveLength(0);
  });
  test('failed save keeps local edits', async () => {
    failSave = true;
    await mount(); await change('Item 1', 'Edited question');
    await blur('Item 1');
    expect(toastError).toHaveBeenCalledWith('Write failed');
    expect(field('Item 1').props.value).toBe('Edited question');
    failSave = false;
    await change('Template', 't1');
    expect(calls('prep_save').at(-1)?.[1]?.items).toEqual([{ ...initial.items[0], text: 'Edited question', byUser: true }]);
    expect(field('Item 1').props.value).toBe('Edited question');
  });
  test('typing an item saves once on blur', async () => {
    await mount();
    const overviewCount = calls('prep_overview').length;
    for (const value of ['a', 'ab', 'abc', 'abcd', 'abcde']) await change('Item 1', value);
    expect(calls('prep_save')).toHaveLength(0);
    expect(calls('prep_overview')).toHaveLength(overviewCount);
    await blur('Item 1');
    expect(calls('prep_save')).toHaveLength(1);
    expect(calls('prep_save')[0][1]?.items).toEqual([{ ...initial.items[0], text: 'abcde', byUser: true }]);
    expect(calls('prep_overview')).toHaveLength(overviewCount + 1);
  });
  test('Enter in an item saves once', async () => {
    await mount();
    await change('Item 1', 'Committed question');
    await keyDown('Item 1', 'Enter');
    expect(calls('prep_save')).toHaveLength(1);
    expect(calls('prep_save')[0][1]?.items).toEqual([{ ...initial.items[0], text: 'Committed question', byUser: true }]);
    await blur('Item 1');
    expect(calls('prep_save')).toHaveLength(1);
  });
  test('new event stays unwritten until an item is added with an empty id', async () => {
    missingPrep = true;
    await mount();
    expect(calls('prep_save')).toHaveLength(0);
    await click('Add item');
    expect(calls('prep_save')[0][1]?.items).toEqual([{ id: '', text: '', kind: 'question', ticked: false, byUser: true }]);
  });
  test('valid txt appends with a blank line and saves', async () => {
    stored.material = 'Existing';
    await mount();
    const file = new File(['Additional notes'], 'notes.txt');
    await act(async () => { await field('Add .txt file').props.onChange({ target: { files: [file], value: 'notes.txt' } }); });
    await flush();
    expect(field('Material').props.value).toBe('Existing\n\nAdditional notes');
    expect(calls('prep_save').at(-1)?.[1]?.material).toBe('Existing\n\nAdditional notes');
  });
  test('txt over the limit is refused', async () => {
    limit = 10;
    stored.material = 'Keep';
    await mount();
    const file = new File(['Additional notes'], 'notes.txt');
    await act(async () => { await field('Add .txt file').props.onChange({ target: { files: [file], value: 'notes.txt' } }); });
    expect(toastError).toHaveBeenCalledWith('22 / 10 characters');
    expect(field('Material').props.value).toBe('Keep');
    expect(calls('prep_save')).toHaveLength(0);
  });
  test('switching event after a failed save keeps the current edits', async () => {
    failSave = true;
    overview.upcoming.push({ ...overview.upcoming[0], uid: 'e2', title: 'Another event' });
    await mount();
    await change('Item 1', 'Keep my edit');
    await blur('Item 1');
    const other = renderer!.root.findAll(n => n.type === 'button' && text(n).includes('Another event'))[0];
    await act(async () => { await other.props.onClick(); });
    expect(field('Item 1').props.value).toBe('Keep my edit');
    expect(calls('prep_get')).toHaveLength(1);
  });
  test('an earlier save never replaces edits made while it was pending', async () => {
    let release!: () => void;
    pauseSave = new Promise<void>(resolve => { release = resolve; });
    await mount();
    await change('Item 1', 'First edit');
    await blur('Item 1');
    await change('Item 1', 'Newest edit');
    await blur('Item 1');
    expect(field('Item 1').props.value).toBe('Newest edit');
    await act(async () => { release(); });
    await flush();
    expect(field('Item 1').props.value).toBe('Newest edit');
    expect(calls('prep_save').at(-1)?.[1]?.items).toEqual([{ ...initial.items[0], text: 'Newest edit', byUser: true }]);
  });
  test('approve saves current material first and shows approval', async () => {
    await mount(); await change('Material', 'Latest material');
    await click('Approve');
    expect(calls('prep_save')[0][1]?.material).toBe('Latest material');
    expect(calls('prep_approve')).toEqual([['prep_approve', { prepId: 1 }]]);
    expect(text(renderer!.root)).toContain('Approved');
  });
  test('delete requires confirmation and clears the editor on success', async () => {
    const originalConfirm = window.confirm;
    try {
      window.confirm = () => false;
      await mount(); await click('Delete prep');
      expect(calls('prep_delete')).toHaveLength(0);
      expect(field('Item 1').props.value).toBe('Original question');
      window.confirm = () => true;
      await click('Delete prep');
      expect(calls('prep_delete')).toEqual([['prep_delete', { prepId: 1 }]]);
      expect(text(renderer!.root)).toContain('Select an event to prepare.');
    } finally { window.confirm = originalConfirm; }
  });

});
