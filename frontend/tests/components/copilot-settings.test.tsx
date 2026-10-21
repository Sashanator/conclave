import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { CopilotSettings } from '../../src/lib/copilot';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  sonner: { ...await import('sonner') },
  select: { ...await import('../../src/components/ui/select') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/components/ui/select', () => originals.select);
});

const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;

const BASE_SETTINGS: CopilotSettings = { provider: null, model: null, answer_language: null, alert_words: [] };
const PIN_KEY = 'summaryLanguageDefault';

let settings: CopilotSettings;
let saveFails: string | null = null;
// When set, the settings reload waits on it, so a test can edit while the reload is pending
let holdLoad: Promise<void> | null = null;

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'copilot_get_settings') {
    await holdLoad;
    return structuredClone(settings);
  }
  if (command === 'copilot_save_settings') {
    if (saveFails) throw new Error(saveFails);
    const next = args?.settings as CopilotSettings;
    // Same rule as the backend: provider and model are both set or both empty
    if ((next.provider === null) !== (next.model === null)) throw new Error('copilot provider and model must both be set or both be empty');
    settings = structuredClone(next);
    return structuredClone(settings);
  }
  if (command === 'api_get_api_key') return args?.provider === 'claude' ? 'sk-test' : '';
  if (command === 'api_get_model_config') return null;
  if (command === 'api_get_custom_openai_config') return null;
  throw new Error(`Unexpected command: ${command}`);
});
const toast = { success: mock(() => {}), error: mock((_what: string, _opts?: unknown) => {}), info: mock(() => {}), warning: mock(() => {}) };

mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('sonner', () => ({ toast }));
// Options are plain elements so a test can read the option shown for the current value
mock.module('../../src/components/ui/select', () => ({
  Select: passThrough,
  SelectContent: passThrough,
  SelectItem: ({ value, children }: { value: string; children?: ReactNode }) => <option value={value}>{children}</option>,
  SelectTrigger: () => null,
  SelectValue: () => null,
}));

const { CopilotSettings: CopilotSettingsPanel } = await import('../../src/components/CopilotSettings');

let renderer: ReactTestRenderer | null = null;

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

async function mount() {
  renderer?.unmount();
  renderer = null;
  await act(async () => {
    renderer = create(<CopilotSettingsPanel />);
  });
  await flush();
}

function text(node: ReactTestInstance): string {
  return node.children.map((child) => (typeof child === 'string' ? child : text(child))).join('');
}

// The text of the option that the select with this trigger id shows for its current value
function shownLabel(triggerId: string): string {
  const select = selectFor(triggerId);
  const option = select.findAll((c) => c.type === 'option' && c.props.value === select.props.value)[0];
  return text(option);
}

// The Select whose trigger has this id
function selectFor(triggerId: string): ReactTestInstance {
  return renderer!.root
    .findAll((n) => n.props.onValueChange !== undefined && n.props.value !== undefined)
    .find((n) => n.findAll((c) => c.props.id === triggerId).length > 0)!;
}

function input(id: string): ReactTestInstance {
  return renderer!.root.findAll((n) => n.type === 'input' && n.props.id === id)[0];
}

function wordInput(): ReactTestInstance {
  return renderer!.root.findAll((n) => n.type === 'input' && n.props['aria-label'] === 'Add an alert word')[0];
}

// Re-queried after each change: a handler captured before a re-render holds stale state
async function typeWord(word: string) {
  await act(async () => {
    wordInput().props.onChange({ target: { value: word } });
  });
  await act(async () => {
    wordInput().props.onKeyDown({ key: 'Enter', preventDefault() {} });
  });
}

function calls(command: string) {
  return invoke.mock.calls.filter(([c]) => c === command);
}

beforeEach(async () => {
  settings = structuredClone(BASE_SETTINGS);
  saveFails = null;
  holdLoad = null;
  window.localStorage.removeItem(PIN_KEY);
  invoke.mockClear();
  toast.error.mockClear();
  await mount();
  await flush();
});

describe('Copilot settings', () => {
  test('language_shows_default_ru', () => {
    expect(shownLabel('copilot-answer-language')).toBe('Russian');
    expect(calls('copilot_save_settings').length).toBe(0);
  });

  test('language_shows_pinned', async () => {
    window.localStorage.setItem(PIN_KEY, 'de');
    await mount();
    expect(shownLabel('copilot-answer-language')).toBe('German');
    expect(calls('copilot_save_settings').length).toBe(0);
  });

  test('a chosen answer language is stored explicitly', async () => {
    await act(async () => {
      renderer!.root.findAll((n) => n.props.onValueChange !== undefined && n.findAll((c) => c.props.id === 'copilot-answer-language').length > 0)[0].props.onValueChange('de');
    });
    await flush();
    const saves = calls('copilot_save_settings');
    expect(saves.length).toBe(1);
    expect((saves[0][1] as { settings: CopilotSettings }).settings.answer_language).toBe('de');
  });

  test('add_and_remove_alert_word', async () => {
    await typeWord('Sasha');
    await flush();
    expect((calls('copilot_save_settings').at(-1)![1] as { settings: CopilotSettings }).settings.alert_words).toEqual(['Sasha']);

    await act(async () => {
      renderer!.root.findAll((n) => n.type === 'button' && n.props['aria-label'] === 'Remove Sasha')[0].props.onClick();
    });
    await flush();
    expect((calls('copilot_save_settings').at(-1)![1] as { settings: CopilotSettings }).settings.alert_words).toEqual([]);
  });

  test('rejects_two_words', async () => {
    await typeWord('Sasha K');
    await flush();
    expect(calls('copilot_save_settings').length).toBe(0);
    expect(wordInput().props.value).toBe('Sasha K');
  });

  test('same_as_summaries_default', () => {
    expect(shownLabel('copilot-provider')).toBe('Same as summaries');
    expect(input('copilot-model').props.disabled).toBe(true);
  });

  test('picking a provider alone does not save; a model with it saves the pair once', async () => {
    await act(async () => {
      selectFor('copilot-provider').props.onValueChange('claude');
    });
    await flush();
    expect(calls('copilot_save_settings').length).toBe(0);
    expect(shownLabel('copilot-provider')).toBe('Claude');
    expect(input('copilot-model').props.disabled).toBe(false);

    await act(async () => {
      input('copilot-model').props.onChange({ target: { value: 'claude-sonnet' } });
    });
    await act(async () => {
      input('copilot-model').props.onBlur();
    });
    await flush();
    const saves = calls('copilot_save_settings');
    expect(saves.length).toBe(1);
    expect((saves[0][1] as { settings: CopilotSettings }).settings).toEqual({ provider: 'claude', model: 'claude-sonnet', answer_language: null, alert_words: [] });
  });

  test('same as summaries saves both empty', async () => {
    settings = { ...BASE_SETTINGS, provider: 'claude', model: 'claude-sonnet' };
    await mount();
    await act(async () => {
      selectFor('copilot-provider').props.onValueChange('same');
    });
    await flush();
    const saves = calls('copilot_save_settings');
    expect(saves.length).toBe(1);
    expect((saves[0][1] as { settings: CopilotSettings }).settings).toEqual({ provider: null, model: null, answer_language: null, alert_words: [] });
  });

  test('a failed save shows the error and keeps the last saved words', async () => {
    saveFails = 'Alert list is full';
    await typeWord('Sasha');
    await flush();
    expect(toast.error).toHaveBeenCalled();
    expect(JSON.stringify(toast.error.mock.calls[0])).toContain('Alert list is full');
    expect(renderer!.root.findAll((n) => n.type === 'button' && n.props['aria-label'] === 'Remove Sasha').length).toBe(0);
  });

  test('an edit made while the reload after a failed save is still saved', async () => {
    saveFails = 'Alert list is full';
    let release!: () => void;
    holdLoad = new Promise<void>((resolve) => { release = resolve; });
    await typeWord('Sasha');
    await flush(); // the save fails and the reload waits
    saveFails = null;
    await typeWord('Anna');
    await flush();
    release();
    holdLoad = null;
    await flush();
    await flush();
    const saves = calls('copilot_save_settings');
    expect((saves.at(-1)![1] as { settings: CopilotSettings }).settings.alert_words).toEqual(['Sasha', 'Anna']);
  });
});
