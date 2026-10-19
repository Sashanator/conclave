import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { DictateSettings, DictateStatus } from '../../src/lib/dictate';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  sonner: { ...await import('sonner') },
  select: { ...await import('../../src/components/ui/select') },
  switchUi: { ...await import('../../src/components/ui/switch') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/components/ui/select', () => originals.select);
  mock.module('../../src/components/ui/switch', () => originals.switchUi);
});

const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
const placeholder = ({ placeholder: text }: { placeholder?: string }) => <>{text}</>;

const BASE_SETTINGS: DictateSettings = {
  enabled: false,
  main_hotkey: 'Ctrl+Win',
  default_mode: 'cleaned-up',
  cleanup_provider: null,
  cleanup_model: null,
  builtin_hotkeys: {},
  modes: [
    { id: 'prompt-for-claude', name: 'Prompt for Claude', instructions: 'Rewrite it.', examples: [], hotkey: 'Ctrl+Win+P' },
  ],
  dictionary: ['Conclave'],
};
const READY_STATUS: DictateStatus = { enabled: false, model_downloaded: true, mic_ok: true, engine_is_whisper: true, reason: null };

let settings: DictateSettings;
let status: DictateStatus;
let saveFails: string | null = null;
let statusFails = false;
// When set, every settings read waits for it, so a test can hold the reload that follows a failed save
let holdLoad: Promise<void> | null = null;

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'dictate_get_settings') {
    if (holdLoad) await holdLoad;
    return structuredClone(settings);
  }
  if (command === 'dictate_status') {
    if (statusFails) throw new Error('status unavailable');
    return status;
  }
  if (command === 'dictate_save_settings') {
    if (saveFails) throw new Error(saveFails);
    settings = args?.settings as DictateSettings;
    return settings;
  }
  if (command === 'api_get_api_key') return args?.provider === 'claude' ? 'sk-test' : '';
  if (command === 'api_get_model_config') return null;
  if (command === 'api_get_custom_openai_config') return null;
  throw new Error(`Unexpected command: ${command}`);
});
const toast = { success: mock(() => {}), error: mock((_what: string, _opts?: unknown) => {}), info: mock(() => {}), warning: mock(() => {}) };

// The Rust side emits `dictate-settings-changed` when the tray changes the default mode
const listeners = new Map<string, (e: { payload: unknown }) => void>();
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('@tauri-apps/api/event', () => ({
  ...originals.event,
  listen: async (event: string, handler: (e: { payload: unknown }) => void) => {
    listeners.set(event, handler);
    // Only removes its own handler: a remount registers before the previous unlisten runs
    return () => {
      if (listeners.get(event) === handler) listeners.delete(event);
    };
  },
}));
mock.module('sonner', () => ({ toast }));
mock.module('../../src/components/ui/select', () => ({
  Select: passThrough,
  SelectContent: passThrough, SelectItem: passThrough, SelectTrigger: passThrough, SelectValue: placeholder,
}));
mock.module('../../src/components/ui/switch', () => ({
  Switch: ({ checked, onCheckedChange, disabled, 'aria-label': label }: { checked?: boolean; onCheckedChange?: (v: boolean) => void; disabled?: boolean; 'aria-label'?: string }) => (
    <button role="switch" aria-label={label} aria-checked={checked} disabled={disabled} onClick={() => onCheckedChange?.(!checked)} />
  ),
}));

const { default: DictatePage } = await import('../../src/app/dictate/page');

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
    renderer = create(<DictatePage />);
  });
  await flush();
}

function text(node: ReactTestInstance): string {
  return node.children.map((child) => (typeof child === 'string' ? child : text(child))).join('');
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

function wordInput(): ReactTestInstance {
  return renderer!.root.findAll((n) => n.type === 'input' && String(n.props.placeholder ?? '').startsWith('Add a word'))[0];
}

// The words shown as chips, from their remove buttons
function chipLabels(): string[] {
  return renderer!.root.findAll((n) => n.type === 'button' && String(n.props['aria-label'] ?? '').startsWith('Remove ')).map((n) => n.props['aria-label'] as string);
}

// The value of the Default mode select
function defaultMode(): string {
  const select = renderer!.root
    .findAll((n) => n.props.onValueChange !== undefined && n.props.value !== undefined)
    .find((n) => n.findAll((c) => c.props.id === 'dictate-default-mode').length > 0);
  return select!.props.value;
}

function calls(command: string) {
  return invoke.mock.calls.filter(([c]) => c === command);
}

beforeEach(async () => {
  settings = structuredClone(BASE_SETTINGS);
  status = { ...READY_STATUS };
  saveFails = null;
  statusFails = false;
  holdLoad = null;
  invoke.mockClear();
  toast.error.mockClear();
  await mount();
});

describe('Dictate page', () => {
  test('renders the General, Modes and Dictionary sections', () => {
    const headings = renderer!.root.findAll((n) => n.type === 'h3').map(text);
    expect(headings).toContain('General');
    expect(headings).toContain('Modes');
    expect(headings).toContain('Dictionary');
  });

  test('the dictionary note says the words are spelling hints for meeting summaries', () => {
    const notes = renderer!.root.findAll((n) => n.type === 'p').map(text);
    expect(notes).toContain('Also used as spelling hints for meeting summaries.');
  });

  test('the switch is disabled with the reason when the engine is not Whisper', async () => {
    status = { ...READY_STATUS, engine_is_whisper: false, reason: 'Dictate needs the Whisper engine.' };
    await mount();
    const toggle = renderer!.root.findAll((n) => n.type === 'button' && n.props['aria-label'] === 'Dictate')[0];
    expect(toggle.props.disabled).toBe(true);
    const reasons = renderer!.root.findAll((n) => n.type === 'p' && text(n) === 'Dictate needs the Whisper engine.');
    expect(reasons.length).toBe(1);
  });

  test('adding a word saves the whole settings with the word appended', async () => {
    
    await typeWord('Tauri');
    await flush();
    const saves = calls('dictate_save_settings');
    expect(saves.length).toBe(1);
    expect((saves[0][1] as { settings: DictateSettings }).settings.dictionary).toEqual(['Conclave', 'Tauri']);
  });

  test('a save error shows a toast and reloads from the backend', async () => {
    saveFails = 'Dictionary is full';
    await typeWord('Tauri');
    await flush();
    expect(toast.error).toHaveBeenCalled();
    expect(JSON.stringify(toast.error.mock.calls[0])).toContain('Dictionary is full');
    expect(calls('dictate_get_settings').length).toBeGreaterThanOrEqual(2);
    // The rejected word is gone from the screen, and only the saved word remains
    expect(chipLabels()).toEqual(['Remove Conclave']);
  });

  test('an edit made while the reload after a failed save is pending is dropped, not saved', async () => {
    let release!: () => void;
    holdLoad = new Promise<void>((resolve) => {
      release = resolve;
    });
    saveFails = 'Dictionary is full';
    await typeWord('Tauri');
    await flush();
    // The reload is still pending: the user types another word
    await typeWord('Good');
    release();
    holdLoad = null;
    await flush();
    await flush();

    const saved = calls('dictate_save_settings').map(([, a]) => (a as { settings: DictateSettings }).settings.dictionary);
    expect(saved).toEqual([['Conclave', 'Tauri']]);
    expect(settings.dictionary).toEqual(['Conclave']);
    expect(chipLabels()).toEqual(['Remove Conclave']);
  });

  test('a default mode changed by the tray replaces the page copy', async () => {
    expect(defaultMode()).toBe('cleaned-up');
    await act(async () => {
      listeners.get('dictate-settings-changed')!({ payload: { ...settings, default_mode: 'as-spoken' } });
    });
    await flush();
    expect(defaultMode()).toBe('as-spoken');
    expect(calls('dictate_save_settings').length).toBe(0);
  });

  test('the switch can still be turned off when the status cannot be read', async () => {
    statusFails = true;
    settings = { ...settings, enabled: true };
    await mount();
    const toggle = renderer!.root.findAll((n) => n.type === 'button' && n.props['aria-label'] === 'Dictate')[0];
    expect(toggle.props.disabled).toBe(false);
  });
});
