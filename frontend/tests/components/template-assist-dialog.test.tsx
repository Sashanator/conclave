import { afterAll, afterEach, beforeEach, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { TemplateJson } from '../../src/lib/template-draft';
import type { AssistResult } from '../../src/lib/templateAssist';

const originals = { core: { ...await import('@tauri-apps/api/core') }, dialog: { ...await import('../../src/components/ui/dialog') } };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('../../src/components/ui/dialog', () => originals.dialog);
});
const pass = ({ children }: { children?: ReactNode }) => <>{children}</>;
mock.module('../../src/components/ui/dialog', () => ({
  Dialog: ({ open, children }: { open: boolean; children?: ReactNode }) => open ? <>{children}</> : null,
  DialogContent: pass, DialogHeader: pass, DialogTitle: pass, DialogDescription: pass, DialogFooter: pass,
}));
const result: AssistResult = {
  template: { name: 'Planning', sections: [{ title: 'Decisions', instruction: 'List decisions' }] },
  changes: [{ field: 'sections', note: 'Added decisions' }], warnings: [],
};
let saved: TemplateJson;
let getSaved: () => Promise<TemplateJson>;
let generate: () => Promise<AssistResult>;
let config: unknown;
let prices: unknown;
let settings: unknown;
const invoke = mock(async (command: string, _args?: unknown): Promise<unknown> => {
  if (command === 'api_get_template') return getSaved();
  if (command === 'api_template_revise') return generate();
  if (command === 'api_template_draft') return generate();
  if (command === 'template_assist_get_settings') return settings;
  if (command === 'api_get_model_config') return config;
  if (command === 'costs_get_prices') return prices;
  throw new Error(`Unexpected command: ${command}`);
});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
const { TemplateAssistDialog } = await import('../../src/components/TemplateAssistDialog');
let renderer: ReactTestRenderer;
const created = mock((_r: AssistResult) => {});
const changed = mock((_open: boolean) => {});
const props = () => ({ open: true, onOpenChange: changed, mode: { kind: 'create' as const }, onCreated: created, onAccepted: () => {} });
const text = (n: ReactTestInstance): string => n.children.map(c => typeof c === 'string' ? c : text(c)).join('');
const button = (label: string) => renderer.root.findAll(n => n.type === 'button' && text(n) === label)[0];
async function mount() { await act(async () => { renderer = create(<TemplateAssistDialog {...props()} />); }); }
async function type(value: string) { await act(async () => renderer.root.findByType('textarea').props.onChange({ target: { value } })); }
async function click() { await act(async () => { button('Generate').props.onClick(); }); }
afterEach(() => { act(() => renderer?.unmount()); });
beforeEach(() => {
  invoke.mockClear(); created.mockClear(); changed.mockClear();
  saved = { name: 'Original', description: 'Keep me', sections: result.template.sections };
  getSaved = async () => saved;
  generate = async () => result; config = undefined; prices = undefined; settings = undefined;
});

test('create calls the draft wrapper once, returns the proposal and closes', async () => {
  await mount();
  expect(renderer.root.findByType('label').children.join('')).toBe('Describe the meeting and the notes you want, or paste an existing template (Notion etc.).');
  expect(button('Generate').props.disabled).toBe(true);
  await type('  '); expect(button('Generate').props.disabled).toBe(true);
  await type('Planning notes'); await click();
  expect(invoke.mock.calls.filter(([c]) => c === 'api_template_draft')).toEqual([['api_template_draft', { description: 'Planning notes' }]]);
  expect(created).toHaveBeenCalledTimes(1); expect(created).toHaveBeenCalledWith(result);
  expect(changed).toHaveBeenCalledWith(false);
});

test('no model errors are inline, with a collapsible raw answer when supplied', async () => {
  generate = async () => { throw 'Set up a summary model in Settings → Summary first.'; };
  await mount(); await type('Planning'); await click();
  expect(text(renderer.root.findByProps({ role: 'alert' }))).toBe('Set up a summary model in Settings → Summary first.');
  expect(created).not.toHaveBeenCalled();
  generate = async () => { throw 'Invalid output after repair\n\nRaw answer:\nnot json'; };
  await click();
  expect(text(renderer.root.findByType('summary'))).toBe('Show raw answer');
  expect(text(renderer.root.findByType('pre'))).toBe('not json');
});

test('closing and reopening drops pending results and errors', async () => {
  for (const fail of [false, true]) {
    let resolve!: (r: AssistResult) => void;
    let reject!: (err: string) => void;
    generate = () => new Promise((res, rej) => { resolve = res; reject = rej; });
    await mount(); await type('Planning'); await click();
    expect(button('Generate').props.disabled).toBe(true);
    await act(async () => renderer.update(<TemplateAssistDialog {...props()} open={false} />));
    await act(async () => renderer.update(<TemplateAssistDialog {...props()} />));
    await act(async () => { if (fail) reject('late error'); else resolve(result); });
    expect(created).not.toHaveBeenCalled(); expect(changed).not.toHaveBeenCalled();
    expect(renderer.root.findAllByProps({ role: 'alert' })).toHaveLength(0);
    act(() => renderer.unmount());
  }
});

test('shows the model alone without prices, then estimates cost for an override', async () => {
  config = { provider: 'openai', model: 'summary-model' };
  await mount();
  expect(text(renderer.root)).toContain('Uses: summary-model (OpenAI)');
  expect(text(renderer.root)).not.toContain('about $');
  act(() => renderer.unmount());
  settings = { provider: 'claude', model: 'assist-model' };
  prices = [{ provider: 'claude', model: 'assist-model', input: 1, output: 2 }];
  await mount(); expect(text(renderer.root)).toContain('Uses: assist-model (Claude) · about $0.005');
});


test('Cancel invalidates a running request immediately and repeated Generate starts only one call', async () => {
  let resolve!: (r: AssistResult) => void;
  generate = () => new Promise(res => { resolve = res; });
  await mount(); await type('Planning');
  await act(async () => {
    button('Generate').props.onClick();
    button('Generate').props.onClick();
  });
  expect(invoke.mock.calls.filter(([c]) => c === 'api_template_draft')).toHaveLength(1);
  await act(async () => { button('Cancel').props.onClick(); });
  expect(changed).toHaveBeenCalledWith(false);
  await act(async () => { resolve(result); });
  expect(created).not.toHaveBeenCalled();
});

const accepted = mock((_merged: TemplateJson, _note: string, _base: TemplateJson) => {});
async function improve(checkStale = true) {
  accepted.mockClear();
  await act(async () => { renderer = create(<TemplateAssistDialog {...props()}
    mode={{ kind: 'improve', templateId: 'planning', saved }} onAccepted={accepted} checkStale={checkStale} />); });
  await type('Keep decisions concise'); await click();
}

test('improve reviews selected fields, warns once per proposal, and returns its base and first note', async () => {
  const base = { ...result.template, name: 'Before', description: 'Hidden description' };
  generate = async () => ({ ...result, base, template: { ...base, name: 'After', style: 'Short' } });
  await improve();
  await act(async () => renderer.root.findByProps({ 'aria-label': 'Name' }).props.onChange({ target: { checked: false } }));
  await act(async () => { button('Accept selected').props.onClick(); });
  expect(accepted).not.toHaveBeenCalled();
  expect(text(renderer.root)).toContain('The template changed since this proposal was made.');
  await act(async () => { button('Accept selected').props.onClick(); });
  expect(accepted).toHaveBeenCalledWith({ ...base, style: 'Short' }, 'Added decisions', base);
  expect(changed).toHaveBeenCalledWith(false);
});

test('failed pre-Accept fetch applies nothing and shows an inline error', async () => {
  await improve();
  getSaved = async () => { throw new Error('Template deleted'); };
  await act(async () => { button('Accept selected').props.onClick(); });
  expect(text(renderer.root.findByProps({ role: 'alert' }))).toContain('Template deleted');
  expect(accepted).not.toHaveBeenCalled();
});

test('refine keeps original request, sends previous, and resets the stale warning', async () => {
  getSaved = async () => ({ ...saved, name: 'Changed elsewhere' });
  await improve();
  await act(async () => { button('Accept selected').props.onClick(); });
  await act(async () => { button('Ask for a different change').props.onClick(); });
  await type('Add owners');
  generate = async () => ({ ...result, template: { ...result.template, style: 'Owners' } });
  await click();
  const calls = invoke.mock.calls.filter(([c]) => c === 'api_template_revise');
  expect(calls[1][1]).toEqual({
    templateId: 'planning', request: 'Keep decisions concise', previous: result, refinement: 'Add owners',
    meetingId: null, includeEdits: false, includeTranscript: false,
  });
  await act(async () => { button('Accept selected').props.onClick(); });
  expect(accepted).not.toHaveBeenCalled();
  expect(text(renderer.root)).toContain('The template changed since this proposal was made.');
});

test('meeting caller can skip the stale fetch', async () => {
  await improve(false);
  await act(async () => { button('Accept selected').props.onClick(); });
  expect(invoke.mock.calls.filter(([c]) => c === 'api_get_template')).toHaveLength(0);
  expect(accepted).toHaveBeenCalledWith(expect.anything(), 'Added decisions', saved);
});

test('Discard during refinement drops its late result', async () => {
  await improve();
  await act(async () => { button('Ask for a different change').props.onClick(); });
  await type('Add owners');
  let resolve!: (r: AssistResult) => void;
  generate = () => new Promise(res => { resolve = res; });
  await click();
  await act(async () => { button('Discard').props.onClick(); });
  await act(async () => { resolve(result); });
  expect(changed).toHaveBeenCalledWith(false);
  expect(accepted).not.toHaveBeenCalled();
});
