import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { DictateSettings, DictateStatus } from '../../src/lib/dictate';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  sonner: { ...await import('sonner') },
  select: { ...await import('../../src/components/ui/select') },
  dropdown: { ...await import('../../src/components/ui/dropdown-menu') },
  switchUi: { ...await import('../../src/components/ui/switch') },
  confirm: { ...await import('../../src/lib/confirm') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/components/ui/select', () => originals.select);
  mock.module('../../src/components/ui/dropdown-menu', () => originals.dropdown);
  mock.module('../../src/components/ui/switch', () => originals.switchUi);
  mock.module('../../src/lib/confirm', () => originals.confirm);
});

const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
const placeholder = ({ placeholder: text }: { placeholder?: string }) => <>{text}</>;

const BASE_SETTINGS: DictateSettings = {
  enabled: false,
  preferred_mic_device: null,
  live_text_style: 'panel',
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
let loadFails = false;
// When set, every settings read waits for it, so a test can hold the reload that follows a failed save
let holdLoad: Promise<void> | null = null;

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'dictate_get_settings') {
    if (holdLoad) await holdLoad;
    if (loadFails) throw new Error('settings unavailable');
    return structuredClone(settings);
  }
  if (command === 'get_audio_devices') return [{ name: 'Dictate mic', device_type: 'Input' }];
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
const confirmAction = mock(async (_opts: unknown) => false);
mock.module('../../src/lib/confirm', () => ({ ...originals.confirm, confirmAction }));
mock.module('sonner', () => ({ toast }));
mock.module('../../src/components/ui/select', () => ({
  Select: passThrough,
  SelectContent: passThrough, SelectItem: passThrough, SelectTrigger: passThrough, SelectValue: placeholder,
}));
// Radix menus open in a portal; show the items in place
mock.module('../../src/components/ui/dropdown-menu', () => ({
  ...originals.dropdown,
  DropdownMenu: passThrough, DropdownMenuTrigger: passThrough, DropdownMenuContent: passThrough,
  DropdownMenuItem: ({ children, onSelect, ...rest }: { children?: ReactNode; onSelect?: () => void; 'aria-label'?: string }) => <button aria-label={rest['aria-label']} onClick={() => onSelect?.()}>{children}</button>,
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

// The id of the mode row that carries the Default badge
function defaultMode(): string {
  return renderer!.root.findAll((n) => n.type === 'li' && n.props['data-mode'] && text(n).includes('Default'))[0].props['data-mode'];
}

function calls(command: string) {
  return invoke.mock.calls.filter(([c]) => c === command);
}

beforeEach(async () => {
  settings = structuredClone(BASE_SETTINGS);
  status = { ...READY_STATUS };
  saveFails = null;
  statusFails = false;
  loadFails = false;
  holdLoad = null;
  invoke.mockClear();
  toast.error.mockClear();
  await mount();
});

describe('Dictate page', () => {
  test('deleting a mode asks first and saves only on yes', async () => {
    const del = () => renderer!.root.findAll((n) => n.type === 'button' && n.props['aria-label'] === 'Delete Prompt for Claude')[0];
    const saves = () => calls('dictate_save_settings').length;
    confirmAction.mockClear();
    confirmAction.mockResolvedValueOnce(false);
    await act(async () => { await del().props.onClick(); });
    await flush();
    expect(confirmAction).toHaveBeenCalledWith({ title: 'Delete the mode "Prompt for Claude"?', confirmLabel: 'Delete', destructive: true });
    expect(saves()).toBe(0);
    confirmAction.mockResolvedValueOnce(true);
    await act(async () => { await del().props.onClick(); });
    await flush();
    expect(saves()).toBe(1);
    expect(settings.modes.some((m) => m.id === 'prompt-for-claude')).toBe(false);
  });

  test('Make default moves the Default badge and saves the default mode', async () => {
    expect(defaultMode()).toBe('cleaned-up');
    const make = (name: string) => renderer!.root.findAll((n) => n.type === 'button' && n.props['aria-label'] === `Make default: ${name}`);
    expect(make('Cleaned up')).toHaveLength(0);
    await act(async () => { make('Prompt for Claude')[0].props.onClick(); });
    await flush();
    expect(settings.default_mode).toBe('prompt-for-claude');
    expect(defaultMode()).toBe('prompt-for-claude');
    expect(make('Cleaned up')).toHaveLength(1);
  });

  test('a mode without a hotkey shows Set hotkey and the chip opens the picker', async () => {
    const chip = () => renderer!.root.findAll((n) => n.type === 'button' && n.props['aria-expanded'] !== undefined && text(n).startsWith('Set hotkey') && text(n).endsWith('As spoken'))[0];
    expect(text(chip())).toBe('Set hotkey for As spoken');
    expect(renderer!.root.findAll((n) => n.props['aria-label'] === 'As spoken hotkey')).toHaveLength(0);
    await act(async () => { chip().props.onClick(); });
    expect(renderer!.root.findAll((n) => n.props['aria-label'] === 'As spoken hotkey').length).toBeGreaterThan(0);
  });

  test('built-in modes sit under a Built in subheading', () => {
    expect(renderer!.root.findAll((n) => n.type === 'h3').map(text)).toEqual(['Built in', 'Your modes']);
  });

  test('while loading the page already has its title', async () => {
    let release!: () => void;
    holdLoad = new Promise<void>((resolve) => { release = resolve; });
    await mount();
    expect(renderer!.root.findAll((n) => n.type === 'h1').map(text)).toEqual(['Dictate']);
    expect(renderer!.root.findAll((n) => n.props['aria-label'] === 'Loading Dictate').length).toBeGreaterThan(0);
    release();
    holdLoad = null;
    await flush();
  });

  test('the cleanup model is set in Models & keys', () => {
    expect(renderer!.root.findAll((n) => n.props.id === 'dictate-cleanup')).toHaveLength(0);
    const link = renderer!.root.findAll((n) => n.type === 'a')[0];
    expect(text(link)).toBe('Change');
    expect(link.props.href).toBe('/settings?tab=models');
    expect(renderer!.root.findAll((n) => n.type === 'p').map(text)).toContain('Used by every mode except "As spoken"');
  });

  test('renders the General, Modes and Dictionary sections', () => {
    const headings = renderer!.root.findAll((n) => n.type === 'h2').map(text);
    expect(headings).toContain('General');
    expect(headings).toContain('Modes');
    expect(headings).toContain('Dictionary');
  });

  test('microphone selection saves a Dictate override and can return to Meetings', async () => {
    const microphone = () => renderer!.root
      .findAll((n) => n.props.onValueChange !== undefined && n.props.value !== undefined)
      .find((n) => n.findAll((c) => c.props.id === 'dictate-microphone').length > 0)!;
    expect(microphone().props.value).toBe('same-as-meetings');
    expect(calls('get_audio_devices').length).toBeGreaterThan(0);
    await act(async () => { microphone().props.onValueChange('Dictate mic (input)'); });
    await flush();
    expect(settings.preferred_mic_device).toBe('Dictate mic (input)');
    await act(async () => { microphone().props.onValueChange('same-as-meetings'); });
    await flush();
    expect(settings.preferred_mic_device).toBeNull();
  });

  test('Live text saves panel or one line', async () => {
    const live = () => renderer!.root
      .findAll((n) => n.props.onValueChange !== undefined && n.props.value !== undefined)
      .find((n) => n.findAll((c) => c.props.id === 'dictate-live-text').length > 0)!;
    expect(live().props.value).toBe('panel');
    await act(async () => { live().props.onValueChange('line'); });
    await flush();
    expect(settings.live_text_style).toBe('line');
  });

  test('the dictionary note says the words are spelling hints for meeting summaries', () => {
    const notes = renderer!.root.findAll((n) => n.type === 'p').map(text);
    expect(notes.filter((n) => n.includes('spelling hints for meeting summaries'))).toHaveLength(1);
  });

  test('the switch is disabled with the reason when the engine is not Whisper', async () => {
    status = { ...READY_STATUS, engine_is_whisper: false, reason: 'Dictate needs the Whisper engine.' };
    await mount();
    const toggle = renderer!.root.findAll((n) => n.type === 'button' && n.props['aria-label'] === 'Enable Dictate')[0];
    expect(toggle.props.disabled).toBe(true);
    const reasons = renderer!.root.findAll((n) => n.type === 'p' && text(n) === 'Dictate needs the Whisper engine.');
    expect(reasons.length).toBe(1);
  });

  test('a rejected word shows its reason and an Add button adds', async () => {
    await typeWord('conclave');
    await flush();
    expect(calls('dictate_save_settings').length).toBe(0);
    const alert = () => renderer!.root.findAll((n) => n.type === 'p' && n.props.role === 'alert').map(text);
    expect(alert()).toEqual(['"conclave" is already in the dictionary']);
    await typeWord('x'.repeat(65));
    expect(alert()).toEqual(['Words can be at most 64 characters']);
    await act(async () => { wordInput().props.onChange({ target: { value: 'Tauri' } }); });
    expect(alert()).toEqual([]);
    const add = renderer!.root.findAll((n) => n.type === 'button' && text(n) === 'Add')[0];
    await act(async () => { add.props.onClick(); });
    await flush();
    expect(settings.dictionary).toEqual(['Conclave', 'Tauri']);
  });

  describe('hotkey capture', () => {
    const record = () => renderer!.root.findAll((n) => n.type === 'button' && n.props['aria-label'] === 'Record shortcut for Main hotkey')[0];
    const ev = (code: string, key: string, mods: Partial<Record<'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey', boolean>> = {}) =>
      ({ code, key, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods, preventDefault() {}, stopPropagation() {} });
    const press = async (kind: 'onKeyDown' | 'onKeyUp', e: ReturnType<typeof ev>) => { await act(async () => { record().props[kind](e); }); await flush(); };
    const statuses = () => renderer!.root.findAll((n) => n.type === 'span' && n.props.role === 'status').map(text);
    const alerts = () => renderer!.root.findAll((n) => n.type === 'p' && n.props.role === 'alert').map(text);

    test('a valid combo saves', async () => {
      await act(async () => { record().props.onClick(); });
      await press('onKeyDown', ev('KeyP', 'p', { ctrlKey: true, altKey: true }));
      expect(settings.main_hotkey).toBe('Ctrl+Alt+P');
      expect(statuses()).toEqual([]);
    });

    test('a modifier-only combo saves when the keys are released', async () => {
      await act(async () => { record().props.onClick(); });
      await press('onKeyDown', ev('ControlLeft', 'Control', { ctrlKey: true }));
      await press('onKeyDown', ev('MetaLeft', 'Meta', { ctrlKey: true, metaKey: true }));
      await press('onKeyUp', ev('MetaLeft', 'Meta', { ctrlKey: true }));
      expect(calls('dictate_save_settings').length).toBe(0);
      await press('onKeyUp', ev('ControlLeft', 'Control'));
      expect(settings.main_hotkey).toBe('Ctrl+Win');
    });

    test('modifiers held with an unsupported key never save on release', async () => {
      await act(async () => { record().props.onClick(); });
      await press('onKeyDown', ev('ControlLeft', 'Control', { ctrlKey: true }));
      await press('onKeyDown', ev('AltLeft', 'Alt', { ctrlKey: true, altKey: true }));
      await press('onKeyDown', ev('ArrowLeft', 'ArrowLeft', { ctrlKey: true, altKey: true }));
      await press('onKeyUp', ev('AltLeft', 'Alt', { ctrlKey: true }));
      await press('onKeyUp', ev('ControlLeft', 'Control'));
      expect(calls('dictate_save_settings').length).toBe(0);
      expect(settings.main_hotkey).toBe('Ctrl+Win');
      expect(alerts()[0]).toContain('is not a supported key');
    });

    test('Esc cancels without saving', async () => {
      await act(async () => { record().props.onClick(); });
      await press('onKeyDown', ev('Escape', 'Escape'));
      expect(calls('dictate_save_settings').length).toBe(0);
      expect(statuses()).toEqual([]);
    });

    test('an unsupported key is rejected and recording goes on', async () => {
      await act(async () => { record().props.onClick(); });
      await press('onKeyDown', ev('Numpad1', '1', { ctrlKey: true }));
      expect(calls('dictate_save_settings').length).toBe(0);
      expect(alerts()[0]).toContain('is not a supported key');
      expect(statuses()).toEqual(['Press the shortcut… Esc cancels']);
    });

    test('a combo the rules refuse shows the reason and Revert restores the saved value', async () => {
      await act(async () => { record().props.onClick(); });
      await press('onKeyDown', ev('KeyP', 'p', { shiftKey: true }));
      expect(calls('dictate_save_settings').length).toBe(0);
      expect(alerts()[0]).toContain('needs Ctrl, Alt or Win');
      const revert = renderer!.root.findAll((n) => n.type === 'button' && text(n) === 'Revert')[0];
      await act(async () => { revert.props.onClick(); });
      expect(alerts()).toEqual([]);
    });

    test('a clash names the other mode', async () => {
      settings.builtin_hotkeys = { 'cleaned-up': 'Ctrl+Alt+P' };
      await mount();
      await act(async () => { record().props.onClick(); });
      await press('onKeyDown', ev('KeyP', 'p', { ctrlKey: true, altKey: true }));
      expect(alerts()[0]).toContain('already used by Cleaned up');
    });
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

  test('a failed save and a failed reload take the rejected word off the screen', async () => {
    saveFails = 'Dictionary is full';
    loadFails = true;
    await typeWord('Tauri');
    await flush();
    expect(toast.error).toHaveBeenCalled();
    expect(calls('dictate_get_settings').length).toBeGreaterThanOrEqual(2);
    // The screen falls back to the last settings the backend confirmed
    expect(chipLabels()).toEqual(['Remove Conclave']);
  });

  test('a failed save and a failed reload keep a default mode the tray changed', async () => {
    await act(async () => {
      listeners.get('dictate-settings-changed')!({ payload: { ...settings, default_mode: 'as-spoken' } });
    });
    await flush();
    saveFails = 'Dictionary is full';
    loadFails = true;
    await typeWord('Tauri');
    await flush();
    expect(defaultMode()).toBe('as-spoken');
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
    const toggle = renderer!.root.findAll((n) => n.type === 'button' && n.props['aria-label'] === 'Enable Dictate')[0];
    expect(toggle.props.disabled).toBe(false);
  });
});
