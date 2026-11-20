import { afterAll, afterEach, beforeEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { Destination, NotionConfig } from '../../src/lib/notion';

const originals = { core: { ...(await import('@tauri-apps/api/core')) }, confirm: { ...(await import('../../src/lib/confirm')) } };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('../../src/lib/confirm', () => originals.confirm);
});

const dest = (id: string, position: number, over: Partial<Destination> = {}): Destination => ({
  id, name: id, colour: position + 1, position, databaseId: 'db-' + id, dataSourceId: null,
  mapping: { titleProp: 'Name', dueProp: null, meetingProp: null, fixed: [] }, lastStatus: null, lastMessage: null, ...over,
});
const prop = (name: string, kind: string, options: string[] = []) => ({ id: name, name, kind, options });
let cfg: NotionConfig;
let tested = false;
let readFails = false;
let confirmOk = false;
const calls: [string, any][] = [];
mock.module('@tauri-apps/api/core', () => ({
  invoke: async (c: string, a?: any) => {
    calls.push([c, a]);
    switch (c) {
      case 'notion_get_config': return structuredClone(tested ? { ...cfg, destinations: cfg.destinations.map((d) => d.id === 'd-personal' ? { ...d, lastStatus: 'not_shared' } : d) } : cfg);
      case 'api_list_templates': return { templates: [{ id: 'standard_meeting', name: 'Standard meeting' }] };
      case 'notion_set_token': cfg.tokenLast4 = a.token.slice(-4); return null;
      case 'notion_test': tested = true; return [{ destId: '', state: 'connected', message: null }, { destId: 'd-personal', state: 'not_shared', message: null }];
      case 'notion_read_columns':
        if (readFails) throw new Error('Notion says no');
        return { databaseId: 'abc-def', dataSourceId: 'ds1', title: 'Tasks', properties: [prop('Name', 'title'), prop('Due', 'date'), prop('Status', 'status', ['To do', 'Done']), prop('Owner', 'people')] };
      default: return null;
    }
  },
}));
mock.module('../../src/lib/confirm', () => ({ ...originals.confirm, confirmAction: async () => confirmOk }));
const { NotionSettings } = await import('../../src/components/Settings/NotionSettings');

let r: ReactTestRenderer;
beforeEach(() => { calls.length = 0; tested = false; readFails = false; confirmOk = false; cfg = { tokenLast4: null, destinations: [dest('d-job', 0), dest('d-personal', 1)], categoryDefaults: [] }; });
afterEach(() => { act(() => r?.unmount()); });
const flush = () => act(async () => { await new Promise((x) => setTimeout(x, 0)); });
const text = (n: any): string => n.children.map((c: any) => (typeof c === 'string' ? c : text(c))).join('');
const render = async () => { await act(async () => { r = create(<NotionSettings />); }); await flush(); };
const btn = (label: string) => r.root.findAll((n) => n.type === 'button' && text(n) === label).slice(-1)[0];
const click = async (label: string) => { await act(async () => { await btn(label).props.onClick(); }); await flush(); };
const pick = async (label: string, value: string) => {
  const sel = r.root.findAll((n) => n.type === 'label' && text(n).startsWith(label))[0].findByType('select');
  await act(async () => { sel.props.onChange({ target: { value } }); });
};
const typeIn = async (label: string, value: string) => {
  const inp = r.root.findAll((n) => n.type === 'label' && text(n).startsWith(label))[0].findByType('input');
  await act(async () => { inp.props.onChange({ target: { value } }); });
};

test('saving a token calls notion_set_token and shows the last four', async () => {
  await render();
  await act(async () => { r.root.findByProps({ id: 'notion-token' }).props.onChange({ target: { value: 'ntn_secret7Qx2' } }); });
  await click('Save');
  expect(calls).toContainEqual(['notion_set_token', { token: 'ntn_secret7Qx2' }]);
  expect(text(r.root)).toContain('Token saved ••••7Qx2');
});

test('Test shows Works and the persisted status; the destId "" entry is no row', async () => {
  cfg.tokenLast4 = '7Qx2';
  await render();
  await click('Test');
  const t = text(r.root);
  expect(t).toContain('Works');
  expect(t).toContain('Not shared with Conclave');
  expect(r.root.findAllByType('tr')).toHaveLength(3); // header + 2 destinations
});

test('add database: read columns, map Due and Status, save', async () => {
  await render();
  await click('Add database');
  await typeIn('Name', 'Job tasks');
  await typeIn('Notion database link', 'https://www.notion.so/abc');
  await click('Read columns');
  const dueSel = r.root.findAll((n) => n.type === 'label' && text(n).startsWith('Due date'))[0].findByType('select');
  expect(dueSel.findAllByType('option').map(text)).toEqual(['None', 'Due']);
  await pick('Due date', 'Due');
  await pick('Status', 'To do');
  expect(text(r.root)).toContain('Not used: Owner');
  await click('Save');
  expect(calls.find((c) => c[0] === 'notion_save_destination')).toEqual(['notion_save_destination', { dest: {
    id: '', name: 'Job tasks', colour: 3, position: 2, databaseId: 'abc-def', dataSourceId: 'ds1',
    mapping: { titleProp: 'Name', dueProp: 'Due', meetingProp: null, fixed: [{ prop: 'Status', kind: 'status', value: 'To do' }] }, lastStatus: null, lastMessage: null } }]);
});

test('Add database is disabled at nine destinations', async () => {
  cfg.destinations = Array.from({ length: 9 }, (_, i) => dest('x' + i, i));
  await render();
  expect(btn('Add database').props.disabled).toBe(true);
});

test('a read-columns failure shows in an alert', async () => {
  readFails = true;
  await render();
  await click('Add database');
  await typeIn('Notion database link', 'x');
  await click('Read columns');
  expect(text(r.root.findByProps({ role: 'alert' }))).toContain('Notion says no');
});

test('Ask me for No template clears the default', async () => {
  await render();
  const sel = r.root.findByProps({ 'aria-label': 'No template default' });
  await act(async () => { sel.props.onChange({ target: { value: '' } }); });
  expect(calls).toContainEqual(['notion_set_category_default', { templateKey: '', destId: null }]);
});

test('Delete confirms first', async () => {
  await render();
  const del = () => r.root.findByProps({ 'aria-label': 'Delete d-job' });
  await act(async () => { await del().props.onClick(); });
  expect(calls.filter((c) => c[0] === 'notion_delete_destination')).toHaveLength(0);
  confirmOk = true;
  await act(async () => { await del().props.onClick(); });
  expect(calls.filter((c) => c[0] === 'notion_delete_destination')).toEqual([['notion_delete_destination', { id: 'd-job' }]]);
});
