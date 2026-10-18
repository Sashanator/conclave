import { afterAll, beforeEach, describe, expect, mock, spyOn, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

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

type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();

type Settings = { enabled: boolean; models_ready: boolean; downloading: boolean; last_analysis: null | { backend: string; run_seconds: number; audio_seconds: number } };
const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
const placeholder = ({ placeholder: text }: { placeholder?: string }) => <>{text}</>;
const SWITCH_LABEL = 'Identify speakers';
const DEFAULT_PEOPLE = [
  { id: 'alex', name: 'Alex', is_self: false, meetings: 3, last_heard: '2026-10-10T09:00:00Z' },
  { id: 'me', name: 'Sasha', is_self: true, meetings: 40, last_heard: '2026-10-16T09:00:00Z' },
  { id: 'dana', name: 'Dana', is_self: false, meetings: 1, last_heard: null },
];

// Mirrors the Rust side closely enough: a download marks the models as downloading until it ends
let settings: Settings;
let people = DEFAULT_PEOPLE;
let holdEnable = false;
let releaseEnable: (() => void) | null = null;
let renameFails = false;

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'speakers_get_settings') return settings;
  if (command === 'people_list') return people;
  if (command === 'speakers_set_enabled') {
    const enabled = Boolean(args?.enabled);
    const apply = () => {
      settings = { ...settings, enabled, downloading: enabled && !settings.models_ready };
    };
    if (holdEnable) {
      return new Promise<void>((resolve) => {
        releaseEnable = () => {
          apply();
          resolve();
        };
      });
    }
    apply();
    return undefined;
  }
  // Progress can arrive before the settings reread says downloading, so the mock leaves downloading false
  if (command === 'speakers_download_models') return undefined;
  if (command === 'speakers_delete_models') return undefined;
  if (command === 'people_rename' && renameFails) throw new Error('name taken');
  if (command === 'people_rename' || command === 'people_merge' || command === 'people_delete') return undefined;
  throw new Error(`Unexpected command: ${command}`);
});
// Set to make that event's listen fail, as the Tauri side can
let refusedEvent: string | null = null;
const listen = mock(async (event: string, handler: Handler) => {
  if (event === refusedEvent) throw new Error('listen refused');
  handlers.set(event, handler);
  // Like Tauri: an unlisten removes only its own registration, never a newer one
  return () => { if (handlers.get(event) === handler) handlers.delete(event); };
});
const toast = { success: mock(() => {}), error: mock((_what: string, _opts?: unknown) => {}), info: mock(() => {}), warning: mock(() => {}) };

mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('@tauri-apps/api/event', () => ({ listen }));
mock.module('sonner', () => ({ toast }));
mock.module('../../src/components/ui/select', () => ({
  Select: ({ children }: { children?: ReactNode }) => <>{children}</>,
  SelectContent: passThrough, SelectItem: passThrough, SelectTrigger: passThrough, SelectValue: placeholder,
}));
mock.module('../../src/components/ui/switch', () => ({
  Switch: ({ checked, onCheckedChange, disabled, 'aria-label': label }: { checked?: boolean; onCheckedChange?: (v: boolean) => void; disabled?: boolean; 'aria-label'?: string }) => (
    <button role="switch" aria-label={label} aria-checked={checked} disabled={disabled} onClick={() => onCheckedChange?.(!checked)} />
  ),
}));

const { SpeakerSettings } = await import('../../src/components/SpeakerSettings');

let renderer: ReactTestRenderer | null = null;

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

async function remount() {
  renderer?.unmount();
  renderer = null;
  await act(async () => {
    renderer = create(<SpeakerSettings />);
  });
  await flush();
}

function text(node: ReactTestInstance): string {
  return node.children.map((child) => (typeof child === 'string' ? child : text(child))).join('');
}

function button(label: string): ReactTestInstance | undefined {
  return renderer!.root.findAll((n) => n.type === 'button' && text(n) === label)[0];
}

function row(id: string): ReactTestInstance {
  return renderer!.root.findAll((n) => n.type === 'li' && n.props['data-person'] === id)[0];
}

function toggle(): ReactTestInstance {
  return renderer!.root.findAll((n) => n.type === 'button' && n.props.role === 'switch' && n.props['aria-label'] === SWITCH_LABEL)[0];
}

function emit(event: string, payload: unknown) {
  return act(async () => {
    handlers.get(event)?.({ payload });
  });
}

// A failed download as the Rust side reports it: the run has ended, with the error in the done event
function failDownload(error: string) {
  settings = { ...settings, downloading: false };
  return emit('speaker-models-done', { ok: false, error });
}

function calls(command: string) {
  return invoke.mock.calls.filter(([name]) => name === command);
}

function renameInput(id: string): ReactTestInstance | undefined {
  return renderer!.root.findAll((n) => n.type === 'input' && n.props.id === `person-name-${id}`)[0];
}

describe('speaker settings tab', () => {
  beforeEach(async () => {
    invoke.mockClear();
    toast.error.mockClear();
    handlers.clear();
    settings = { enabled: false, models_ready: false, downloading: false, last_analysis: null };
    people = DEFAULT_PEOPLE;
    holdEnable = false;
    releaseEnable = null;
    renameFails = false;
    await remount();
  });

  test('toggle on invokes speakers_set_enabled with { enabled: true }', async () => {
    expect(toggle().props['aria-checked']).toBe(false);
    await act(async () => {
      toggle().props.onClick();
    });
    await flush();
    expect(calls('speakers_set_enabled')).toEqual([['speakers_set_enabled', { enabled: true }]]);
  });

  test('the switch is disabled while speakers_set_enabled is in flight', async () => {
    holdEnable = true;
    await act(async () => {
      toggle().props.onClick();
    });
    await flush();
    expect(toggle().props.disabled).toBe(true);
    await act(async () => {
      releaseEnable?.();
    });
    await flush();
    expect(toggle().props.disabled).toBe(false);
    expect(toggle().props['aria-checked']).toBe(true);
  });

  test('the download progress event renders a percentage', async () => {
    await act(async () => {
      toggle().props.onClick();
    });
    await flush();
    await emit('speaker-models-progress', { percent: 42, downloaded_bytes: 42, total_bytes: 100 });
    expect(renderer!.root.findAll((n) => n.type === 'span' && text(n) === '42%').length).toBe(1);
  });

  test('a failed download shows the error and Retry, which invokes speakers_download_models', async () => {
    await act(async () => {
      toggle().props.onClick();
    });
    await flush();
    await failDownload('network down');
    await flush();
    expect(renderer!.root.findAll((n) => n.type === 'p' && text(n) === 'network down').length).toBe(1);

    await act(async () => {
      button('Retry')!.props.onClick();
    });
    await flush();
    expect(calls('speakers_download_models').length).toBe(1);
  });

  test('after a restart with the feature on and no models, Retry shows without a stored error', async () => {
    settings = { enabled: true, models_ready: false, downloading: false, last_analysis: null };
    await remount();
    expect(renderer!.root.findAll((n) => n.type === 'p' && text(n) === "Speaker models aren't downloaded yet.").length).toBe(1);
    expect(button('Retry')).toBeDefined();

    await act(async () => {
      button('Retry')!.props.onClick();
    });
    await flush();
    expect(calls('speakers_download_models').length).toBe(1);
  });

  test('Retry is not shown while the models are ready or the feature is off', async () => {
    settings = { enabled: true, models_ready: true, downloading: false, last_analysis: null };
    await remount();
    expect(button('Retry')).toBeUndefined();
    settings = { enabled: false, models_ready: false, downloading: false, last_analysis: null };
    await remount();
    expect(button('Retry')).toBeUndefined();
  });

  test('progress after Retry shows the bar even before settings report downloading', async () => {
    await act(async () => {
      toggle().props.onClick();
    });
    await flush();
    await failDownload('network down');
    await flush();
    await act(async () => {
      button('Retry')!.props.onClick();
    });
    await flush();
    await emit('speaker-models-progress', { percent: 55, downloaded_bytes: 55, total_bytes: 100 });
    expect(renderer!.root.findAll((n) => n.type === 'span' && text(n) === '55%').length).toBe(1);
  });

  test('self has no Delete and no Merge, and is pinned first with "this is you"', async () => {
    const rows = renderer!.root.findAll((n) => n.type === 'li');
    expect(rows.map((r) => r.props['data-person'])).toEqual(['me', 'alex', 'dana']);
    const self = row('me');
    expect(text(self)).toContain('this is you');
    expect(text(self)).toContain('Rename');
    expect(text(self)).not.toContain('Delete');
    expect(text(self)).not.toContain('Merge into');
    expect(text(row('alex'))).toContain('Delete');
    expect(text(row('alex'))).toContain('Merge into');
  });

  test('Delete confirms with the exact text and then invokes people_delete', async () => {
    const confirm = spyOn(window, 'confirm').mockImplementation(() => true);
    await act(async () => {
      row('alex').findAll((n) => n.type === 'button' && n.props['aria-label'] === 'Delete Alex')[0].props.onClick();
    });
    await flush();
    expect(confirm.mock.calls).toEqual([['Voiceprints are removed; past transcripts keep the name']]);
    expect(calls('people_delete')).toEqual([['people_delete', { id: 'alex' }]]);
    confirm.mockRestore();
  });

  test('a cancelled Delete invokes nothing', async () => {
    const confirm = spyOn(window, 'confirm').mockImplementation(() => false);
    await act(async () => {
      row('alex').findAll((n) => n.type === 'button' && text(n) === 'Delete')[0].props.onClick();
    });
    await flush();
    expect(calls('people_delete').length).toBe(0);
    confirm.mockRestore();
  });

  test('Rename submits the trimmed name with people_rename, with a length limit', async () => {
    await act(async () => {
      row('alex').findAll((n) => n.type === 'button' && text(n) === 'Rename')[0].props.onClick();
    });
    expect(renameInput('alex')!.props.maxLength).toBe(80);
    await act(async () => {
      renameInput('alex')!.props.onChange({ target: { value: '  Alexei ' } });
    });
    await act(async () => {
      row('alex').findAll((n) => n.type === 'form')[0].props.onSubmit({ preventDefault() {} });
    });
    await flush();
    expect(calls('people_rename')).toEqual([['people_rename', { id: 'alex', name: 'Alexei' }]]);
    expect(renameInput('alex')).toBeUndefined();
  });

  test('Save is disabled for a blank name', async () => {
    await act(async () => {
      row('alex').findAll((n) => n.type === 'button' && text(n) === 'Rename')[0].props.onClick();
    });
    await act(async () => {
      renameInput('alex')!.props.onChange({ target: { value: '   ' } });
    });
    expect(button('Save')!.props.disabled).toBe(true);
  });

  test('a failed rename keeps the editor open with the typed name', async () => {
    renameFails = true;
    await act(async () => {
      row('alex').findAll((n) => n.type === 'button' && text(n) === 'Rename')[0].props.onClick();
    });
    await act(async () => {
      renameInput('alex')!.props.onChange({ target: { value: 'Alexei' } });
    });
    await act(async () => {
      row('alex').findAll((n) => n.type === 'form')[0].props.onSubmit({ preventDefault() {} });
    });
    await flush();
    expect(toast.error.mock.calls.length).toBe(1);
    expect(renameInput('alex')!.props.value).toBe('Alexei');
  });

  test('Escape cancels the rename', async () => {
    await act(async () => {
      row('alex').findAll((n) => n.type === 'button' && text(n) === 'Rename')[0].props.onClick();
    });
    await act(async () => {
      renameInput('alex')!.props.onKeyDown({ key: 'Escape' });
    });
    expect(renameInput('alex')).toBeUndefined();
    expect(calls('people_rename').length).toBe(0);
  });

  test('Merge into resets to the placeholder, is labelled per person, and invokes people_merge', async () => {
    const trigger = row('alex').findAll((n) => n.props['aria-label'] === 'Merge Alex into…')[0];
    expect(trigger).toBeDefined();
    const select = row('alex').findAll((n) => typeof n.props.onValueChange === 'function')[0];
    expect(select.props.value).toBe('');
    await act(async () => {
      select.props.onValueChange('dana');
    });
    await flush();
    expect(calls('people_merge')).toEqual([['people_merge', { from: 'alex', into: 'dana' }]]);
  });

  test('Delete models invokes speakers_delete_models and is shown only when the models are ready', async () => {
    expect(button('Delete models')).toBeUndefined();
    settings = { enabled: true, models_ready: true, downloading: false, last_analysis: null };
    await remount();
    await act(async () => {
      button('Delete models')!.props.onClick();
    });
    await flush();
    expect(calls('speakers_delete_models').length).toBe(1);
  });

  test('a refused listen does not keep the other listeners from being removed', async () => {
    refusedEvent = 'speaker-models-done';
    const logged = spyOn(console, 'error').mockImplementation(() => {});
    try {
      await remount();
    } finally {
      refusedEvent = null;
      logged.mockRestore();
    }
    expect(handlers.has('speaker-models-progress')).toBe(true);
    expect(handlers.has('speaker-settings-changed')).toBe(true);
    await act(async () => { renderer!.unmount(); });
    renderer = null;
    expect(handlers.has('speaker-models-progress')).toBe(false);
    expect(handlers.has('speaker-settings-changed')).toBe(false);
  });

  test('shows the last analysis line when the settings have one', async () => {
    settings = { enabled: true, models_ready: true, downloading: false, last_analysis: { backend: 'directml', run_seconds: 41, audio_seconds: 3500 } };
    await remount();
    expect(renderer!.root.findAll((n) => n.type === 'p' && text(n) === 'Last analysis: DirectML · 41 s for a 58-min meeting').length).toBe(1);
  });
});
