import { afterAll, beforeEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingPrep } from '../../src/lib/prep';
import { SUGGEST_MESSAGE } from '../../src/lib/prep';

const originals = { core: { ...await import('@tauri-apps/api/core') }, sonner: { ...await import('sonner') } };
afterAll(() => {
  renderer?.unmount();
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
});
const base: MeetingPrep = {
  id: 7, eventUid: null, eventStart: null, eventTitle: 'Interview', templateId: null, material: '', chat: [],
  items: [{ id: 'a', text: 'Ask about team', kind: 'question', ticked: false, byUser: false }],
  approvedAt: null, createdAt: '2026-10-29T10:00:00Z', updatedAt: '2026-10-29T10:00:00Z',
};
let stored: MeetingPrep | null;
let failSend = false;
let failSave = false;
const toastError = mock((_m: unknown) => {});
const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  switch (command) {
    case 'prep_for_meeting': return stored && structuredClone(stored);
    case 'prep_material_limit': return 100;
    case 'prep_save':
      if (failSave) throw new Error('Write failed');
      stored = { ...(stored ?? base), id: 7, material: args!.material as string, items: args!.items as MeetingPrep['items'] };
      return structuredClone(stored);
    case 'prep_send':
      if (failSend) throw new Error('Send failed');
      stored!.chat = [{ role: 'user', text: args!.message as string, at: 'now' }, { role: 'assistant', text: 'Ask about budget', at: 'now' }];
      return { prep: structuredClone(stored), listUpdated: true };
    case 'prep_approve': return { ...stored, approvedAt: stored!.updatedAt };
    default: return undefined;
  }
});
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));
mock.module('sonner', () => ({ ...originals.sonner, toast: { ...originals.sonner.toast, error: toastError } }));
const { PrepTab } = await import('../../src/components/MeetingPage/PrepTab');
let renderer: ReactTestRenderer | null = null;
const text = (n: ReactTestInstance): string => n.children.map((c) => typeof c === 'string' ? c : text(c)).join('');
const button = (name: string) => renderer!.root.findAll((n) => n.type === 'button' && text(n).trim() === name)[0];
const field = (label: string) => renderer!.root.find((n) => n.props['aria-label'] === label && typeof n.type === 'string');
const calls = (c: string) => invoke.mock.calls.filter((x) => x[0] === c);
const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const click = async (name: string) => { await act(async () => { await button(name).props.onClick(); }); await flush(); };
const change = async (label: string, value: string) => { await act(async () => { field(label).props.onChange({ target: { value } }); }); };
async function mount(open = true) {
  await act(async () => { renderer = create(<PrepTab meetingId="m1" />); });
  await flush();
  if (open) await click('Prep material and chat');
}
beforeEach(() => {
  renderer?.unmount(); renderer = null;
  stored = structuredClone(base); failSend = false; failSave = false;
  invoke.mockClear(); toastError.mockClear();
});

test('the chat is collapsed until the disclosure opens', async () => {
  await mount(false);
  expect(renderer!.root.findAll((n) => n.props['aria-label'] === 'Material')).toHaveLength(0);
  expect(button('Prep material and chat').props['aria-expanded']).toBe(false);
  await click('Prep material and chat');
  expect(field('Material')).toBeDefined();
});

test('Suggest saves the material, sends it and shows the reply', async () => {
  await mount();
  expect(button('Suggest')).toBeUndefined();
  await change('Material', 'Agenda text');
  await click('Suggest');
  expect(calls('prep_save')[0][1]).toMatchObject({ meetingId: 'm1', material: 'Agenda text', eventTitle: 'Interview' });
  expect(calls('prep_send')).toEqual([['prep_send', { prepId: 7, message: SUGGEST_MESSAGE }]]);
  expect(text(renderer!.root)).toContain('Ask about budget');
});

test('a meeting without a prep gets one on the first save', async () => {
  stored = null;
  await mount();
  await change('Material', 'Notes');
  await click('Suggest');
  expect(calls('prep_save')).toHaveLength(1);
  expect(calls('prep_send')).toHaveLength(1);
});

test('Send posts the typed message and clears the draft', async () => {
  await mount();
  await change('Message', 'What else?');
  await click('Send');
  expect(calls('prep_send')[0][1]).toEqual({ prepId: 7, message: 'What else?' });
  expect(field('Message').props.value).toBe('');
});

test('a failed send shows a toast and keeps the message', async () => {
  failSend = true;
  await mount();
  await change('Message', 'Keep me');
  await click('Send');
  expect(toastError).toHaveBeenCalledWith('Send failed');
  expect(field('Message').props.value).toBe('Keep me');
});

test('a failed save shows a toast and sends nothing', async () => {
  failSave = true;
  await mount();
  await change('Message', 'Keep me');
  await change('Material', 'Changed');
  await click('Send');
  expect(toastError).toHaveBeenCalledWith('Write failed');
  expect(calls('prep_send')).toHaveLength(0);
  expect(field('Message').props.value).toBe('Keep me');
});

test('over the limit blocks sending and says so', async () => {
  await mount();
  await change('Material', 'x'.repeat(101));
  expect(text(renderer!.root)).toContain('101 / 100 characters');
  expect(button('Send').props.disabled).toBe(true);
});

test('Approve saves the material first, writes the checklist approval and shows it', async () => {
  await mount();
  await change('Material', 'Agenda');
  await click('Approve');
  expect(calls('prep_save')).toHaveLength(1);
  expect(calls('prep_approve')).toEqual([['prep_approve', { prepId: 7 }]]);
  expect(text(renderer!.root)).toContain('Approved');
});

test('a failed load keeps the chat away so nothing is saved over the prep', async () => {
  invoke.mockImplementationOnce(async () => { throw new Error('read failed'); });
  await mount(false);
  expect(toastError).toHaveBeenCalled();
  expect(renderer!.root.findAll((n) => n.type === 'button' && text(n).trim() === 'Prep material and chat')).toHaveLength(0);
});
