import { afterAll, beforeEach, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  sonner: { ...await import('sonner') },
  select: { ...await import('../../src/components/ui/select') },
  config: { ...await import('../../src/contexts/ConfigContext') },
  smodel: { ...await import('../../src/components/SummaryModelSettings') },
  slang: { ...await import('../../src/components/SummaryLanguageSettings') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/components/ui/select', () => originals.select);
  mock.module('../../src/contexts/ConfigContext', () => originals.config);
  mock.module('../../src/components/SummaryModelSettings', () => originals.smodel);
  mock.module('../../src/components/SummaryLanguageSettings', () => originals.slang);
});

const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
let customConfig: unknown = null;
let keySaveFails = false;
let knowledge: Record<string, unknown>;
const fixtures = (): Record<string, unknown> => ({
  api_get_api_key: null,
  api_get_model_config: { provider: 'openai', model: 'gpt-4o' },
  copilot_get_settings: { provider: 'claude', model: 'claude-sonnet-5-5', answer_language: 'de', alert_words: ['x'] },
  knowledge_get_settings: knowledge,
  template_assist_get_settings: { provider: 'openai', model: 'gpt-5', includeTranscript: true, hideEditHint: false },
  dictate_get_settings: { cleanup_provider: null, cleanup_model: null, dictionary: [] },
  practice_get_settings: { chat_model: 'claude-sonnet-5-5', plan_model: 'claude-opus-5-5', plan_effort: 'high' },
  costs_get_prices: [],
});
const invoke = mock(async (cmd: string, args?: any): Promise<unknown> => {
  if (cmd === 'api_get_api_key') return args.provider === 'claude' ? 'sk-c' : '';
  if (cmd === 'api_get_custom_openai_config') return customConfig;
  if (cmd === 'api_save_provider_key' && keySaveFails) throw new Error('disk');
  if (cmd.includes('_save_')) return null;
  return fixtures()[cmd] ?? null;
});
const emit = mock(async () => {});
const toast = { success: mock(() => {}), error: mock((_a: string, _b?: unknown) => {}), info: mock(() => {}), warning: mock(() => {}) };
const updateProviderApiKey = mock((_p: string, _k: string | null) => {});
let auto = true;
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));
mock.module('@tauri-apps/api/event', () => ({ ...originals.event, emit, listen: async () => () => {} }));
mock.module('sonner', () => ({ toast }));
mock.module('../../src/contexts/ConfigContext', () => ({ ...originals.config, useConfig: () => ({ updateProviderApiKey, isAutoSummary: auto, toggleIsAutoSummary() {} }) }));
mock.module('../../src/components/SummaryModelSettings', () => ({ SummaryModelSettings: () => <div>stub:summary-editor</div> }));
mock.module('../../src/components/SummaryLanguageSettings', () => ({ SummaryLanguageSettings: () => <div>stub:summary-language</div> }));
mock.module('../../src/components/ui/select', () => ({
  Select: passThrough,
  SelectContent: passThrough,
  SelectItem: ({ value, children }: { value: string; children?: ReactNode }) => <option value={value}>{children}</option>,
  SelectTrigger: () => null,
  SelectValue: () => null,
}));

const { ModelsKeysPane } = await import('../../src/components/Settings/ModelsKeysPane');
const { SummariesPane } = await import('../../src/components/Settings/SummariesPane');

let r: ReactTestRenderer;
const flush = () => act(async () => { await new Promise((x) => setTimeout(x, 0)); });
const mount = async (el: JSX.Element) => { await act(async () => { r = create(el); }); await flush(); await flush(); };
const text = (n: ReactTestInstance): string => n.children.map((c) => (typeof c === 'string' ? c : text(c))).join('');
const row = (label: string) => r.root.findAll((n) => n.type === 'div' && n.props.id === `setting-${label.toLowerCase().replace(/[^a-z0-9]+/g, '-')}`)[0];
const button = (scope: ReactTestInstance, name: string) => scope.findAll((n) => n.type === 'button' && text(n) === name)[0];
const names = () => invoke.mock.calls.map((c) => c[0] as string);
const shown = (triggerId: string) => {
  const sel = r.root.findAll((n) => n.props.onValueChange !== undefined && n.props.value !== undefined).find((n) => n.findAll((c) => c.props.id === triggerId).length > 0)!;
  return text(sel.findAll((c) => c.type === 'option' && c.props.value === sel.props.value)[0]);
};

beforeEach(() => {
  invoke.mockClear(); emit.mockClear(); toast.error.mockClear(); updateProviderApiKey.mockClear();
  customConfig = null; keySaveFails = false; auto = true;
  knowledge = { provider: null, model: null, my_names: ['Sasha'], extract_enabled: true, digest_enabled: false, calendar_host: 'c.example', calendar_marker: 'x' };
});

test('mount writes nothing; headings and provider status', async () => {
  await mount(<ModelsKeysPane />);
  expect(names().filter((n) => n.includes('_save_'))).toEqual([]);
  const h2 = r.root.findAllByType('h2').map(text);
  expect(h2).toEqual(expect.arrayContaining(['Providers', 'Which model each feature uses', 'Prices (per 1M tokens)', 'Model prices']));
  expect(text(row('Claude'))).toContain('Key set');
  expect(button(row('Claude'), 'Replace')).toBeTruthy();
  expect(text(row('OpenAI'))).toContain('Not set');
  expect(button(row('OpenAI'), 'Add key')).toBeTruthy();
  expect(text(row('Custom server'))).toContain('Set it up in the Summary row');
  expect(row('Custom server').findAllByType('button')).toHaveLength(0);
});

test('adding a key saves only the key, then shows Key set and Saved', async () => {
  await mount(<ModelsKeysPane />);
  await act(async () => { button(row('OpenAI'), 'Add key').props.onClick(); });
  const input = () => r.root.findByProps({ 'aria-label': 'OpenAI API key' });
  expect(input().props.type).toBe('password');
  await act(async () => { input().props.onChange({ target: { value: 'sk-o' } }); });
  await act(async () => { await button(row('OpenAI'), 'Save').props.onClick(); });
  await flush(); await flush();
  expect(invoke).toHaveBeenCalledWith('api_save_provider_key', { provider: 'openai', apiKey: 'sk-o' });
  expect(names()).not.toContain('api_save_model_config');
  expect(updateProviderApiKey).toHaveBeenCalledWith('openai', 'sk-o');
  expect(text(row('OpenAI'))).toContain('Saved ✓');
  expect(r.root.findAllByProps({ 'aria-label': 'OpenAI API key' })).toHaveLength(0);
});

test('a failed key save toasts and keeps the typed value', async () => {
  keySaveFails = true;
  await mount(<ModelsKeysPane />);
  await act(async () => { button(row('OpenAI'), 'Add key').props.onClick(); });
  await act(async () => { r.root.findByProps({ 'aria-label': 'OpenAI API key' }).props.onChange({ target: { value: 'sk-o' } }); });
  await act(async () => { await button(row('OpenAI'), 'Save').props.onClick(); });
  await flush();
  expect(toast.error).toHaveBeenCalled();
  expect(r.root.findByProps({ 'aria-label': 'OpenAI API key' }).props.value).toBe('sk-o');
  expect(updateProviderApiKey).not.toHaveBeenCalled();
});

test('custom server row shows host and Replace key', async () => {
  customConfig = { endpoint: 'https://llm.example:8080/v1' };
  await mount(<ModelsKeysPane />);
  expect(text(row('Custom server'))).toContain('llm.example:8080');
  expect(button(row('Custom server'), 'Replace')).toBeTruthy();
});

test('feature rows show fixtures; a model pick saves through knowledge_save_settings keeping my_names', async () => {
  await mount(<ModelsKeysPane />);
  expect(shown('mk-copilot-provider')).toBe('Claude');
  expect(shown('mk-knowledge-provider')).toBe('Same as Summary');
  expect(shown('mk-template-provider')).toBe('OpenAI');
  expect(shown('mk-dictate-provider')).toBe('Same as Summary');
  const sel = r.root.findAll((n) => n.props.onValueChange !== undefined && n.findAll((c) => c.props.id === 'mk-knowledge-provider').length > 0)[0];
  await act(async () => { sel.props.onValueChange('claude'); });
  expect(names().filter((n) => n.includes('_save_'))).toEqual([]); // provider alone is pending
  const model = r.root.findByProps({ id: 'mk-knowledge-model' });
  await act(async () => { model.props.onChange({ target: { value: 'claude-haiku-5-5' } }); });
  await act(async () => { r.root.findByProps({ id: 'mk-knowledge-model' }).props.onBlur(); });
  await flush(); await flush();
  const save = invoke.mock.calls.find((c) => c[0] === 'knowledge_save_settings')!;
  expect((save[1] as any).input).toMatchObject({ provider: 'claude', model: 'claude-haiku-5-5', my_names: ['Sasha'] });
  expect(text(row('Tasks & digests'))).toContain('Saved ✓');
});

test('practice row shows its selects with the fixture values and no Same as Summary', async () => {
  await mount(<ModelsKeysPane />);
  expect(shown('practice-chat-model')).toBe('Claude Sonnet 5.5');
  expect(shown('practice-plan-model')).toBe('Claude Opus 5.5');
  expect(shown('practice-plan-effort')).toBe('High');
  expect(text(row('Practice'))).not.toContain('Same as Summary');
});

test('SummariesPane: auto summary switch, language, link to models', async () => {
  await mount(<SummariesPane />);
  const sw = r.root.findByProps({ 'aria-label': 'Auto summary' });
  expect(sw).toBeTruthy();
  expect(text(r.root)).toContain('Generate the summary when a recording stops');
  expect(text(r.root)).toContain('Summary language');
  expect(text(row('Summary model'))).toContain('Set in Models & keys');
});
