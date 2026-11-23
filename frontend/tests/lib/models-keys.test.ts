import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';

const originals = { core: { ...(await import('@tauri-apps/api/core')) }, event: { ...(await import('@tauri-apps/api/event')) } };

afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
});

const invoke = mock(async (_c: string, _a?: Record<string, unknown>): Promise<unknown> => null);
const emit = mock(async (_e: string, _p?: unknown): Promise<void> => {});
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));
mock.module('@tauri-apps/api/event', () => ({ ...originals.event, emit }));

const { saveProviderKey, loadFeatureModels, saveFeatureModel, featureChoice } = await import('../../src/lib/models-keys');

beforeEach(() => {
  invoke.mockReset();
  emit.mockReset();
});

describe('saveProviderKey', () => {
  test('saves, reads the config back and emits it', async () => {
    const cfg = { provider: 'claude', model: 'm' };
    invoke.mockImplementation(async (c) => (c === 'api_get_model_config' ? cfg : null));
    await saveProviderKey('claude', 'sk');
    expect(invoke.mock.calls).toEqual([['api_save_provider_key', { provider: 'claude', apiKey: 'sk' }], ['api_get_model_config']]);
    expect(emit.mock.calls).toEqual([['model-config-updated', cfg]]);
  });

  test('no emit when there is no config', async () => {
    invoke.mockImplementation(async () => null);
    await saveProviderKey('claude', 'sk');
    expect(emit).not.toHaveBeenCalled();
  });

  test('a stored key is a success even when the config re-read fails', async () => {
    invoke.mockImplementation(async (c) => {
      if (c === 'api_get_model_config') throw new Error('summary key unreadable');
      return null;
    });
    await expect(saveProviderKey('claude', 'sk')).resolves.toBeUndefined();
    expect(emit).not.toHaveBeenCalled();
  });

  test('a failed save rejects and does not emit', async () => {
    invoke.mockImplementation(async () => {
      throw new Error('nope');
    });
    await expect(saveProviderKey('claude', 'sk')).rejects.toThrow('nope');
    expect(emit).not.toHaveBeenCalled();
  });
});

const dictate = { enabled: true, preferred_mic_device: null, live_text_style: 'panel', main_hotkey: 'Ctrl+Alt+D', default_mode: 'cleaned-up', cleanup_provider: 'groq', cleanup_model: null, builtin_hotkeys: {}, modes: [], dictionary: ['Conclave'] };
const copilot = { provider: 'claude', model: 'claude-sonnet-5-5', answer_language: 'de', alert_words: ['Sasha'] };
const knowledge = { provider: null, model: null, my_names: ['Sasha'], extract_enabled: true, digest_enabled: false, calendar_host: 'cal.example', calendar_marker: '\u{1F4DE}' };
const assist = { provider: 'openai', model: 'gpt-5', includeTranscript: true, hideEditHint: true };
const reads: Record<string, unknown> = {
  copilot_get_settings: copilot,
  knowledge_get_settings: knowledge,
  template_assist_get_settings: assist,
  dictate_get_settings: dictate,
};
const route = async (c: string) => reads[c] ?? null;
const names = () => invoke.mock.calls.map((x) => x[0]);

describe('feature models', () => {
  test('models_keys_preserves_choices: load', async () => {
    invoke.mockImplementation(route);
    expect(await loadFeatureModels()).toEqual({
      copilot: { provider: 'claude', model: 'claude-sonnet-5-5' },
      knowledge: null,
      templateAssist: { provider: 'openai', model: 'gpt-5' },
      dictateCleanup: null,
    });
    expect(names().filter((n) => n.includes('_save_'))).toEqual([]);
  });

  test('copilot save keeps answer language and alert words, reads first', async () => {
    invoke.mockImplementation(route);
    await saveFeatureModel('copilot', { provider: 'openai', model: 'gpt-5' });
    expect(invoke.mock.calls).toEqual([
      ['copilot_get_settings'],
      ['copilot_save_settings', { settings: { provider: 'openai', model: 'gpt-5', answer_language: 'de', alert_words: ['Sasha'] } }],
    ]);
  });

  test('knowledge save drops calendar_host', async () => {
    invoke.mockImplementation(route);
    await saveFeatureModel('knowledge', { provider: 'groq', model: 'llama' });
    expect(invoke.mock.calls).toEqual([
      ['knowledge_get_settings'],
      ['knowledge_save_settings', { input: { provider: 'groq', model: 'llama', my_names: ['Sasha'], extract_enabled: true, digest_enabled: false, calendar_marker: '\u{1F4DE}' } }],
    ]);
  });

  test('template assistant null choice clears the pair only', async () => {
    invoke.mockImplementation(route);
    await saveFeatureModel('templateAssist', null);
    expect(invoke.mock.calls).toEqual([
      ['template_assist_get_settings'],
      ['template_assist_save_settings', { settings: { provider: null, model: null, includeTranscript: true, hideEditHint: true } }],
    ]);
  });

  test('dictate cleanup changes only the cleanup pair', async () => {
    invoke.mockImplementation(route);
    await saveFeatureModel('dictateCleanup', { provider: 'claude', model: 'claude-haiku-5-5' });
    expect(invoke.mock.calls).toEqual([
      ['dictate_get_settings'],
      ['dictate_save_settings', { settings: { ...dictate, cleanup_provider: 'claude', cleanup_model: 'claude-haiku-5-5' } }],
    ]);
  });

  test('a save error propagates', async () => {
    invoke.mockImplementation(async (c) => {
      if (c.endsWith('_save_settings')) throw new Error('disk');
      return reads[c] ?? null;
    });
    await expect(saveFeatureModel('copilot', null)).rejects.toThrow('disk');
  });

  test('featureChoice', () => {
    expect(featureChoice(' claude ', ' m ')).toEqual({ provider: 'claude', model: 'm' });
    for (const [p, m] of [['groq', null], [null, 'm'], ['', 'm'], ['claude', '  ']] as const) expect(featureChoice(p, m)).toBeNull();
  });
});
