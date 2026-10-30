import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  select: { ...await import('../../src/components/ui/select') },
  switchUi: { ...await import('../../src/components/ui/switch') },
  sonner: { ...await import('sonner') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('../../src/components/ui/select', () => originals.select);
  mock.module('../../src/components/ui/switch', () => originals.switchUi);
  mock.module('sonner', () => originals.sonner);
});

const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;

type AssistSettings = { provider: string | null; model: string | null; includeTranscript: boolean; hideEditHint: boolean };

let settings: AssistSettings;
// Per test: whether saving the settings fails
let saveFails: boolean;

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'template_assist_get_settings') return structuredClone(settings);
  if (command === 'template_assist_save_settings') {
    if (saveFails) throw 'disk full';
    settings = structuredClone(args?.settings as AssistSettings);
    return undefined;
  }
  if (command === 'api_get_api_key') return '';
  if (command === 'api_get_model_config') return null;
  if (command === 'api_get_custom_openai_config') return null;
  throw new Error(`Unexpected command: ${command}`);
});

const toastError = mock((_message: string, _options?: unknown) => {});

mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('sonner', () => ({ toast: { error: toastError } }));
mock.module('../../src/components/ui/select', () => ({
  Select: passThrough,
  SelectContent: passThrough,
  SelectItem: passThrough,
  SelectTrigger: () => null,
  SelectValue: () => null,
}));
// A plain button stands in for the Radix switch, so a test can press it
mock.module('../../src/components/ui/switch', () => ({
  Switch: ({ checked, onCheckedChange, ...rest }: { checked?: boolean; onCheckedChange?: (on: boolean) => void; [key: string]: unknown }) => (
    <button type="button" role="switch" aria-checked={checked} onClick={() => onCheckedChange?.(!checked)} {...rest} />
  ),
}));

const { TemplateAssistSettings } = await import('../../src/components/TemplateAssistSettings');

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
    renderer = create(<TemplateAssistSettings />);
  });
  await flush();
}

function textOf(node: ReactTestInstance): string {
  return node.children.map((c) => (typeof c === 'string' ? c : textOf(c))).join('');
}

function byLabel(label: string): ReactTestInstance {
  const found = renderer!.root.findAll((n) => n.props['aria-label'] === label && typeof n.type === 'string');
  if (found.length === 0) throw new Error(`no element labelled "${label}"`);
  return found[0];
}

function labelWith(text: string): ReactTestInstance {
  const found = renderer!.root.findAll((n) => n.type === 'label' && textOf(n) === text);
  if (found.length === 0) throw new Error(`no label with "${text}"`);
  return found[0];
}

beforeEach(() => {
  settings = { provider: null, model: null, includeTranscript: false, hideEditHint: true };
  saveFails = false;
  invoke.mockClear();
  toastError.mockClear();
});

describe('TemplateAssistSettings', () => {
  test('shows the model picker label and an unchecked transcript switch', async () => {
    await mount();
    expect(labelWith('Model for the template assistant')).toBeDefined();
    const sw = byLabel('Include the transcript by default');
    expect(sw.props.id).toBe('template-assist-transcript');
    expect(sw.props['aria-checked']).toBe(false);
  });

  test('toggling the transcript switch saves the whole settings object', async () => {
    await mount();
    await act(async () => {
      byLabel('Include the transcript by default').props.onClick();
    });
    await flush();
    expect(invoke).toHaveBeenCalledWith('template_assist_save_settings', {
      settings: { provider: null, model: null, includeTranscript: true, hideEditHint: true },
    });
  });

  test('a failed save shows a toast and reloads the saved settings', async () => {
    saveFails = true;
    await mount();
    const gets = () => invoke.mock.calls.filter(([c]) => c === 'template_assist_get_settings').length;
    const before = gets();
    await act(async () => {
      byLabel('Include the transcript by default').props.onClick();
    });
    await flush();
    expect(toastError).toHaveBeenCalledWith('Template assistant settings were not saved', expect.anything());
    expect(gets()).toBe(before + 1);
  });
});
