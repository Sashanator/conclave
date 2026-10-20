import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { PracticeSettings } from '../../src/lib/practice';

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
const placeholder = ({ placeholder: text }: { placeholder?: string }) => <>{text}</>;

const DEFAULTS: PracticeSettings = { chat_model: 'claude-haiku-5-5', plan_model: 'claude-opus-5-5', plan_effort: 'high' };

let settings: PracticeSettings;
let saveFails: string | null = null;
let loadFails = false;

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'practice_get_settings') {
    if (loadFails) throw new Error('settings unavailable');
    return structuredClone(settings);
  }
  if (command === 'practice_save_settings') {
    if (saveFails) throw new Error(saveFails);
    settings = args?.settings as PracticeSettings;
    return settings;
  }
  throw new Error(`Unexpected command: ${command}`);
});
const toast = { success: mock(() => {}), error: mock((_what: string, _opts?: unknown) => {}), info: mock(() => {}), warning: mock(() => {}) };

mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('sonner', () => ({ toast }));
mock.module('../../src/components/ui/select', () => ({
  Select: passThrough,
  SelectContent: passThrough, SelectItem: passThrough, SelectTrigger: passThrough, SelectValue: placeholder,
}));

const { ModelsCard } = await import('../../src/components/Practice/ModelsCard');

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
    renderer = create(<ModelsCard />);
  });
  await flush();
}

function text(node: ReactTestInstance): string {
  return node.children.map((child) => (typeof child === 'string' ? child : text(child))).join('');
}

// The Select whose trigger carries this aria-label. Re-queried after each change.
function selectOf(label: string): ReactTestInstance {
  return renderer!.root
    .findAll((n) => n.props.onValueChange !== undefined && n.props.value !== undefined)
    .find((n) => n.findAll((c) => c.props['aria-label'] === label).length > 0)!;
}

function paragraphs(): string[] {
  return renderer!.root.findAll((n) => n.type === 'p').map(text);
}

function calls(command: string) {
  return invoke.mock.calls.filter(([c]) => c === command);
}

beforeEach(async () => {
  settings = structuredClone(DEFAULTS);
  saveFails = null;
  loadFails = false;
  invoke.mockClear();
  toast.error.mockClear();
  await mount();
});

describe('Models card', () => {
  test('models_card_loads_and_saves: shows the saved models, costs them, and saves each change', async () => {
    expect(selectOf('Conversation').props.value).toBe('claude-haiku-5-5');
    expect(selectOf('Plan and report').props.value).toBe('claude-opus-5-5');
    expect(selectOf('Effort').props.value).toBe('high');
    expect(paragraphs()).toContain('≈ $0.04 per 30-min session');
    expect(paragraphs()).toContain('≈ $0.64 per 30-min session');

    await act(async () => selectOf('Conversation').props.onValueChange('claude-sonnet-5-5'));
    await flush();
    await act(async () => selectOf('Effort').props.onValueChange('xhigh'));
    await flush();

    const saves = calls('practice_save_settings');
    expect(saves.length).toBe(2);
    expect((saves[1][1] as { settings: PracticeSettings }).settings).toEqual({
      chat_model: 'claude-sonnet-5-5', plan_model: 'claude-opus-5-5', plan_effort: 'xhigh',
    });
    expect(selectOf('Conversation').props.value).toBe('claude-sonnet-5-5');
    expect(paragraphs()).toContain('≈ $0.77 per 30-min session');
    expect(paragraphs()).toContain('≈ $0.84 per 30-min session');
  });

  test('effort_changes_the_plan_and_report_figure: the Effort row has no cost line of its own', async () => {
    expect(paragraphs().filter((p) => p.startsWith('≈'))).toEqual(['≈ $0.04 per 30-min session', '≈ $0.64 per 30-min session']);

    await act(async () => selectOf('Effort').props.onValueChange('medium'));
    await flush();
    expect(paragraphs()).toContain('≈ $0.54 per 30-min session');
    expect(paragraphs().filter((p) => p.startsWith('≈'))).toHaveLength(2);
  });

  test('changing_the_plan_and_report_model_updates_its_figure_and_saves', async () => {
    await act(async () => selectOf('Plan and report').props.onValueChange('claude-sonnet-5-5'));
    await flush();
    expect(paragraphs()).toContain('≈ $0.32 per 30-min session');
    const saves = calls('practice_save_settings');
    expect((saves.at(-1)![1] as { settings: PracticeSettings }).settings.plan_model).toBe('claude-sonnet-5-5');
  });

  test('a failed first load says so under the Models heading', async () => {
    loadFails = true;
    await mount();
    const headings = renderer!.root.findAll((n) => n.type === 'h3').map(text);
    expect(headings).toEqual(['Models']);
    expect(paragraphs()).toContain('Could not load model settings');
  });

  test('a save error shows a toast, reloads the saved models and drops the edit', async () => {
    saveFails = 'Model not allowed';
    await act(async () => selectOf('Conversation').props.onValueChange('claude-opus-5-5'));
    await flush();
    expect(toast.error).toHaveBeenCalled();
    expect(JSON.stringify(toast.error.mock.calls[0])).toContain('Model not allowed');
    expect(calls('practice_get_settings').length).toBeGreaterThanOrEqual(2);
    expect(selectOf('Conversation').props.value).toBe('claude-haiku-5-5');
  });

  test('opus_chat_shows_slower_replies: only when the conversation model is Opus', async () => {
    expect(paragraphs()).not.toContain('Slower replies');

    await act(async () => selectOf('Conversation').props.onValueChange('claude-opus-5-5'));
    await flush();
    expect(paragraphs()).toContain('Slower replies');
    expect(paragraphs()).toContain('≈ $1.54 per 30-min session');

    await act(async () => selectOf('Conversation').props.onValueChange('claude-sonnet-5-5'));
    await flush();
    expect(paragraphs()).not.toContain('Slower replies');
  });
});
