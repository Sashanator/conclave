import { afterAll, afterEach, beforeEach, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
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
let generate: () => Promise<AssistResult>;
let config: unknown;
let prices: unknown;
let settings: unknown;
const invoke = mock(async (command: string, _args?: unknown): Promise<unknown> => {
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
