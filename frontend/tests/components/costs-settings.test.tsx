import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import type { ReactTestInstance, ReactTestRenderer } from 'react-test-renderer';
import { act, create } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  select: { ...await import('../../src/components/ui/select') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  renderer?.unmount();
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('../../src/components/ui/select', () => originals.select);
});

const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
const placeholder = ({ placeholder: text }: { placeholder?: string }) => <>{text}</>;

interface Price {
  provider: string;
  model: string;
  input: number;
  output: number;
  cache_read: number;
  cache_write: number;
  built_in: boolean;
}

const BUILT_IN: Price[] = [
  { provider: 'claude', model: 'claude-haiku-5-5', input: 0.1, output: 0.5, cache_read: 0.01, cache_write: 0.125, built_in: true },
  { provider: 'claude', model: 'claude-sonnet-5-5', input: 2, output: 10, cache_read: 0.2, cache_write: 2.5, built_in: true },
];
const USER_ROW: Price = { provider: 'ollama', model: 'llama3', input: 0, output: 0, cache_read: 0, cache_write: 0, built_in: false };

let prices: Price[];
let loadFails = false;
const MAX_MESSAGE = 'each price must be from 0 to 1000 $ per million tokens';

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'costs_get_prices') {
    if (loadFails) throw new Error('store unreadable');
    return structuredClone(prices);
  }
  if (command === 'costs_save_prices') {
    const rows = args?.rows as Price[];
    if (rows.some((r) => r.input < 0 || r.output < 0 || r.cache_read < 0 || r.cache_write < 0)) throw new Error(MAX_MESSAGE);
    prices = structuredClone(rows);
    return structuredClone(prices);
  }
  if (command === 'api_get_api_key') return null;
  if (command === 'api_get_model_config') return null;
  if (command === 'api_get_custom_openai_config') return null;
  throw new Error(`Unexpected command: ${command} ${JSON.stringify(args ?? {})}`);
});

mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('../../src/components/ui/select', () => ({
  Select: passThrough,
  SelectContent: passThrough,
  SelectItem: passThrough,
  SelectTrigger: passThrough,
  SelectValue: placeholder,
}));

const { CostsSettings } = await import('../../src/components/CostsSettings');

let renderer: ReactTestRenderer | null = null;

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

function labelOf(node: ReactTestInstance | string): string {
  if (typeof node === 'string') return node;
  return node.children.map(labelOf).join('');
}

function byLabel(label: string): ReactTestInstance {
  const found = renderer!.root.findAll((n) => n.props['aria-label'] === label && typeof n.type === 'string');
  if (found.length !== 1) throw new Error(`expected one element labelled "${label}", found ${found.length}`);
  return found[0];
}

async function editPrice(label: string, value: string) {
  await act(async () => {
    byLabel(label).props.onChange({ target: { value } });
  });
  await act(async () => {
    byLabel(label).props.onBlur();
  });
  await flush();
}

beforeEach(async () => {
  prices = structuredClone([...BUILT_IN, USER_ROW]);
  loadFails = false;
  invoke.mockClear();
  renderer?.unmount();
  renderer = null;
  await act(async () => {
    renderer = create(<CostsSettings />);
  });
  await flush();
});

describe('costs settings', () => {
  test('an edit sends the whole list with only that price changed', async () => {
    await editPrice('Input price for claude claude-haiku-5-5', '0.2');

    const saves = invoke.mock.calls.filter(([command]) => command === 'costs_save_prices');
    expect(saves).toHaveLength(1);
    const sent = (saves[0][1] as { rows: Price[] }).rows;
    expect(sent).toEqual([
      { ...BUILT_IN[0], input: 0.2 },
      BUILT_IN[1],
      USER_ROW,
    ]);
  });

  test('a negative price shows the error inline', async () => {
    await editPrice('Input price for claude claude-haiku-5-5', '-1');

    expect(labelOf(renderer!.root)).toContain(MAX_MESSAGE);
    expect(prices[0].input).toBe(0.1);
  });

  test('a built-in row has no delete button, an added row does', () => {
    expect(renderer!.root.findAll((n) => n.props['aria-label'] === 'Delete claude claude-haiku-5-5' && typeof n.type === 'string')).toHaveLength(0);
    expect(renderer!.root.findAll((n) => n.props['aria-label'] === 'Delete ollama llama3' && typeof n.type === 'string')).toHaveLength(1);
  });

  test('deleting an added row sends the rest of the list', async () => {
    await act(async () => {
      byLabel('Delete ollama llama3').props.onClick();
    });
    await flush();

    expect(prices.map((r) => `${r.provider}/${r.model}`)).toEqual(['claude/claude-haiku-5-5', 'claude/claude-sonnet-5-5']);
  });

  test('when the list fails to load, nothing can be saved over it', async () => {
    loadFails = true;
    renderer?.unmount();
    await act(async () => {
      renderer = create(<CostsSettings />);
    });
    await flush();

    expect(labelOf(renderer!.root)).toContain('Could not load model prices');
    expect(renderer!.root.findAll((n) => n.props['aria-label'] === 'Input price for claude claude-haiku-5-5' && typeof n.type === 'string')).toHaveLength(0);
    expect(invoke.mock.calls.some(([command]) => command === 'costs_save_prices')).toBe(false);
  });
});
