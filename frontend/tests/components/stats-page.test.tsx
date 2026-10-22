import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactTestInstance, ReactTestRenderer } from 'react-test-renderer';
import { act, create } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  renderer?.unmount();
  localStorage.clear();
  mock.module('@tauri-apps/api/core', () => originals.core);
});

const cost = (start: string, total: number, providers: unknown[] = []) => ({ start, total_usd: total, providers });
const providerRow = (provider: string, input: number, output: number, costUsd: number, unpriced = 0) => ({
  provider,
  input_tokens: input,
  output_tokens: output,
  cost_usd: costUsd,
  unpriced_calls: unpriced,
});

const COST_WEEK = [
  cost('2026-10-19', 0.012, [providerRow('claude', 18400, 2100, 0.012), providerRow('ollama', 500, 90, 0, 2)]),
  cost('2026-10-12', 0.5),
];
const COST_MONTH = [cost('2026-10-01', 1.27, [providerRow('claude', 1_250_000, 1000, 1.27)])];
const TALK = {
  current: [{ template_id: 'tpl-standup', template_name: 'Standup', meetings: 3, self_seconds: 30, total_seconds: 100 }],
  previous: [{ template_id: 'tpl-standup', template_name: 'Standup', meetings: 2, self_seconds: 40, total_seconds: 100 }],
};
const MEETINGS = [{ meeting_id: 'm-1', title: 'Daily sync', date: '2026-10-20', self_seconds: 30, total_seconds: 100 }];

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'stats_cost') return args?.period === 'month' ? structuredClone(COST_MONTH) : structuredClone(COST_WEEK);
  if (command === 'stats_talk_time') return structuredClone(TALK);
  if (command === 'stats_talk_time_meetings') return structuredClone(MEETINGS);
  throw new Error(`Unexpected command: ${command} ${JSON.stringify(args ?? {})}`);
});

mock.module('@tauri-apps/api/core', () => ({ invoke }));

const { default: StatsPage } = await import('../../src/app/stats/page');

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

function buttonWith(text: string): ReactTestInstance {
  const found = renderer!.root.findAll((n) => n.type === 'button' && labelOf(n).includes(text));
  if (found.length === 0) throw new Error(`no button with "${text}"`);
  return found[0];
}

function pageText(): string {
  return labelOf(renderer!.root);
}

beforeEach(async () => {
  localStorage.clear();
  invoke.mockClear();
  renderer?.unmount();
  renderer = null;
  await act(async () => {
    renderer = create(<StatsPage />);
  });
  await flush();
});

describe('stats page', () => {
  test('renders the current cost, provider rows and unpriced count', () => {
    const text = pageText();
    expect(text).toContain('$0.012');
    expect(text).toContain('claude');
    expect(text).toContain('Uncached input 18.4k');
    expect(text).toContain('Output 2.1k');
    expect(text).toContain('2 unpriced');
    expect(text).toContain('$0.500');
  });

  test('the period toggle refetches with the month period and remembers it', async () => {
    await act(async () => {
      buttonWith('Month').props.onClick();
    });
    await flush();

    expect(invoke).toHaveBeenCalledWith('stats_cost', { period: 'month', count: 13 });
    expect(invoke).toHaveBeenCalledWith('stats_talk_time', { period: 'month' });
    expect(localStorage.getItem('conclave.statsPeriod')).toBe('month');
    expect(pageText()).toContain('$1.270');
  });

  test('clicking a talk-time type lists its meetings', async () => {
    await act(async () => {
      buttonWith('Standup').props.onClick();
    });
    await flush();

    expect(invoke).toHaveBeenCalledWith('stats_talk_time_meetings', { period: 'week', back: 0, templateId: 'tpl-standup' });
    expect(pageText()).toContain('Daily sync');
  });
});
