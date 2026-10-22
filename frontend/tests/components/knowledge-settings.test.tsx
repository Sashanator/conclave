import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  select: { ...await import('../../src/components/ui/select') },
  switchUi: { ...await import('../../src/components/ui/switch') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('../../src/components/ui/select', () => originals.select);
  mock.module('../../src/components/ui/switch', () => originals.switchUi);
});

const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;

type View = {
  provider: string | null;
  model: string | null;
  my_names: string[];
  extract_enabled: boolean;
  digest_enabled: boolean;
  calendar_host: string | null;
};

let view: View;

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'knowledge_get_settings') return structuredClone(view);
  if (command === 'knowledge_save_settings') {
    const input = args?.input as Omit<View, 'calendar_host'>;
    view = { ...structuredClone(input), calendar_host: view.calendar_host };
    return structuredClone(view);
  }
  if (command === 'api_get_api_key') return '';
  if (command === 'api_get_model_config') return null;
  if (command === 'api_get_custom_openai_config') return null;
  throw new Error(`Unexpected command: ${command}`);
});

mock.module('@tauri-apps/api/core', () => ({ invoke }));
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

const { KnowledgeSettings } = await import('../../src/components/KnowledgeSettings');

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
    renderer = create(<KnowledgeSettings />);
  });
  await flush();
}

function textOf(node: ReactTestInstance): string {
  return node.children.map((c) => (typeof c === 'string' ? c : textOf(c))).join('');
}

function allText(): string {
  return renderer!.root.findAll(() => true).filter((n) => typeof n.type === 'string').map((n) => n.children.filter((c) => typeof c === 'string').join('')).join(' ');
}

function byLabel(label: string): ReactTestInstance {
  const found = renderer!.root.findAll((n) => n.props['aria-label'] === label && typeof n.type === 'string');
  if (found.length === 0) throw new Error(`no element labelled "${label}"`);
  return found[0];
}

beforeEach(() => {
  view = {
    provider: null,
    model: null,
    my_names: ['Sasha'],
    extract_enabled: true,
    digest_enabled: true,
    calendar_host: 'calendar.example.com',
  };
  invoke.mockClear();
});

describe('KnowledgeSettings', () => {
  test('toggling extraction saves extract_enabled false', async () => {
    await mount();
    await act(async () => {
      await byLabel('Extract my tasks after each summary').props.onClick();
    });
    await flush();
    expect(invoke).toHaveBeenCalledWith(
      'knowledge_save_settings',
      { input: expect.objectContaining({ extract_enabled: false, digest_enabled: true }) },
    );
  });

  test('adding the name Саша saves it', async () => {
    await mount();
    await act(async () => {
      byLabel('Add a name').props.onChange({ target: { value: 'Саша' } });
    });
    await act(async () => {
      await byLabel('Add a name').props.onKeyDown({ key: 'Enter', preventDefault() {} });
    });
    await flush();
    expect(invoke).toHaveBeenCalledWith(
      'knowledge_save_settings',
      { input: expect.objectContaining({ my_names: ['Sasha', 'Саша'] }) },
    );
  });

  test('the view never shows the calendar URL, only the host', async () => {
    view.calendar_host = 'calendar.example.com';
    await mount();
    const text = allText();
    expect(text).toContain('calendar.example.com');
    expect(text).not.toContain('http');
    expect(text).not.toContain('://');
  });

  test('the calendar card says Not connected without a host', async () => {
    view.calendar_host = null;
    await mount();
    expect(allText()).toContain('Not connected');
  });
});
