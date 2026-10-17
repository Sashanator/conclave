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

const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
const placeholder = ({ placeholder: text }: { placeholder?: string }) => <>{text}</>;
const SWITCH_LABEL = 'Identify speakers';

let settings = { enabled: false, models_ready: false, downloading: false, last_analysis: null as null | { backend: string; run_seconds: number; audio_seconds: number } };
let people = [
  { id: 'alex', name: 'Alex', is_self: false, meetings: 3, last_heard: '2026-10-10T09:00:00Z' },
  { id: 'me', name: 'Sasha', is_self: true, meetings: 40, last_heard: '2026-10-16T09:00:00Z' },
  { id: 'dana', name: 'Dana', is_self: false, meetings: 1, last_heard: null },
];

const invoke = mock(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'speakers_get_settings') return settings;
  if (command === 'people_list') return people;
  if (command === 'speakers_set_enabled') {
    settings = { ...settings, enabled: Boolean(_args?.enabled), downloading: Boolean(_args?.enabled) };
    return undefined;
  }
  if (command === 'speakers_download_models') return undefined;
  if (command === 'speakers_delete_models') return undefined;
  if (command === 'people_rename' || command === 'people_merge' || command === 'people_delete') return undefined;
  throw new Error(`Unexpected command: ${command}`);
});
const listen = mock(async (event: string, handler: Handler) => {
  handlers.set(event, handler);
  return () => handlers.delete(event);
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

function text(node: ReactTestInstance): string {
  return node.children.map((child) => (typeof child === 'string' ? child : text(child))).join('');
}

function button(label: string): ReactTestInstance {
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

function calls(command: string) {
  return invoke.mock.calls.filter(([name]) => name === command);
}

describe('speaker settings tab', () => {
  beforeEach(async () => {
    invoke.mockClear();
    toast.error.mockClear();
    handlers.clear();
    settings = { enabled: false, models_ready: false, downloading: false, last_analysis: null };
    people = [
      { id: 'alex', name: 'Alex', is_self: false, meetings: 3, last_heard: '2026-10-10T09:00:00Z' },
      { id: 'me', name: 'Sasha', is_self: true, meetings: 40, last_heard: '2026-10-16T09:00:00Z' },
      { id: 'dana', name: 'Dana', is_self: false, meetings: 1, last_heard: null },
    ];
    renderer?.unmount();
    renderer = null;
    await act(async () => {
      renderer = create(<SpeakerSettings />);
    });
    await flush();
  });

  test('toggle on invokes speakers_set_enabled with { enabled: true }', async () => {
    expect(toggle().props['aria-checked']).toBe(false);
    await act(async () => {
      toggle().props.onClick();
    });
    await flush();
    expect(calls('speakers_set_enabled')).toEqual([['speakers_set_enabled', { enabled: true }]]);
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
    await emit('speaker-models-done', { ok: false, error: 'network down' });
    await flush();
    expect(renderer!.root.findAll((n) => n.type === 'p' && text(n) === 'network down').length).toBe(1);

    await act(async () => {
      button('Retry').props.onClick();
    });
    await flush();
    expect(calls('speakers_download_models').length).toBe(1);
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
      row('alex').findAll((n) => n.type === 'button' && text(n) === 'Delete')[0].props.onClick();
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

  test('Rename shows an inline input and saves the trimmed name with people_rename', async () => {
    await act(async () => {
      row('alex').findAll((n) => n.type === 'button' && text(n) === 'Rename')[0].props.onClick();
    });
    await act(async () => {
      renderer!.root.findAll((n) => n.type === 'input' && n.props.id === 'person-name-alex')[0].props.onChange({ target: { value: '  Alexei ' } });
    });
    await act(async () => {
      row('alex').findAll((n) => n.type === 'button' && text(n) === 'Save')[0].props.onClick();
    });
    await flush();
    expect(calls('people_rename')).toEqual([['people_rename', { id: 'alex', name: 'Alexei' }]]);
  });

  test('Merge into picks a target and invokes people_merge with from and into', async () => {
    const select = row('alex').findAll((n) => typeof n.props.onValueChange === 'function')[0];
    await act(async () => {
      select.props.onValueChange('dana');
    });
    await flush();
    expect(calls('people_merge')).toEqual([['people_merge', { from: 'alex', into: 'dana' }]]);
  });

  test('Delete models invokes speakers_delete_models', async () => {
    await act(async () => {
      button('Delete models').props.onClick();
    });
    await flush();
    expect(calls('speakers_delete_models').length).toBe(1);
  });

  test('shows the last analysis line when the settings have one', async () => {
    settings = { enabled: true, models_ready: true, downloading: false, last_analysis: { backend: 'directml', run_seconds: 41, audio_seconds: 3500 } };
    await act(async () => {
      renderer!.unmount();
      renderer = create(<SpeakerSettings />);
    });
    await flush();
    expect(renderer!.root.findAll((n) => n.type === 'p' && text(n) === 'Last analysis: DirectML · 41 s for a 58-min meeting').length).toBe(1);
  });
});
