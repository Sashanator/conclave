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
  if (command === 'stats_cost') return args?.period === 'month' ? structuredClone(COST_MONTH) : structuredClone(COST_WEEK_OVERRIDE ?? COST_WEEK);
  if (command === 'stats_talk_time') return structuredClone(TALK);
  if (command === 'stats_talk_time_meetings') return structuredClone(MEETINGS);
  throw new Error(`Unexpected command: ${command} ${JSON.stringify(args ?? {})}`);
});

mock.module('@tauri-apps/api/core', () => ({ invoke }));

const { formatLongDate } = await import('../../src/lib/dates');
const { readOrigin } = await import('../../src/lib/back-nav');
const { default: StatsPage } = await import('../../src/app/stats/page');

let renderer: ReactTestRenderer | null = null;
let COST_WEEK_OVERRIDE: unknown[] | null = null;

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
    expect(text).toContain('$0.01');
    expect(text).not.toContain('$0.012');
    expect(text).toContain('claude');
    expect(text).toContain('Uncached input 18.4k');
    expect(text).toContain('Output 2.1k');
    expect(text).toContain('2 unpriced');
  });

  test('the period toggle refetches with the month period and remembers it', async () => {
    await act(async () => {
      buttonWith('Month').props.onClick();
    });
    await flush();

    expect(invoke).toHaveBeenCalledWith('stats_cost', { period: 'month', count: 13 });
    expect(invoke).toHaveBeenCalledWith('stats_talk_time', { period: 'month' });
    expect(localStorage.getItem('conclave.statsPeriod')).toBe('month');
    expect(pageText()).toContain('$1.27');
  });

  test('the page title is one h1 "Stats" and the frame is not centred', () => {
    expect(renderer!.root.findAll((n) => n.type === 'h1').map(labelOf)).toEqual(['Stats']);
    const cls = renderer!.root.findAll((n) => typeof n.props.className === 'string' && /mx-auto|max-w-3xl/.test(n.props.className));
    expect(cls).toHaveLength(0);
  });

  test('the Week/Month toggle sits in the frame actions', async () => {
    const group = renderer!.root.findAll((n) => n.props.role === 'group' && n.props['aria-label'] === 'Period');
    expect(group.length).toBeGreaterThan(0);
    await act(async () => {
      buttonWith('Month').props.onClick();
    });
    await flush();
    expect(localStorage.getItem('conclave.statsPeriod')).toBe('month');
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

describe('talk time', () => {
  test('talk time is a table with an explained share column', () => {
    const ths = renderer!.root.findAll((n) => n.type === 'th').map(labelOf);
    expect(renderer!.root.findAll((n) => n.type === 'table')).toHaveLength(1);
    for (const h of ['Category', 'Meetings', 'Previous week']) expect(ths.some((t) => t.includes(h))).toBe(true);
    expect(ths.some((t) => t.includes('Your share of talk time') && t.includes('how much of the talking was you'))).toBe(true);
  });

  test('each row shows a share bar sized to the share', () => {
    const row = renderer!.root.findAll((n) => n.type === 'tr' && labelOf(n).includes('Standup'))[0];
    expect(labelOf(row)).toContain('30%');
    expect(labelOf(row)).toContain('40%');
    expect(row.findAll((n) => (n.props.style as { width?: string } | undefined)?.width === '30%').length).toBeGreaterThan(0);
  });

  test('meeting rows show a long date and remember the origin', async () => {
    await act(async () => {
      buttonWith('Standup').props.onClick();
    });
    await flush();
    expect(buttonWith('Standup').props['aria-expanded']).toBe(true);
    expect(pageText()).toContain(formatLongDate(new Date('2026-10-20T00:00')));
    expect(pageText()).not.toContain('2026-10-20');
    const link = renderer!.root.findAll((n) => typeof n.props.href === 'string' && n.props.href.includes('meeting-details'))[0];
    link.props.onClick();
    expect(readOrigin()).toEqual({ path: '/stats', label: 'Stats' });
  });

  test('month view says Previous month', async () => {
    await act(async () => {
      buttonWith('Month').props.onClick();
    });
    await flush();
    expect(renderer!.root.findAll((n) => n.type === 'th').map(labelOf)).toContain('Previous month');
  });
});

describe('cost chart', () => {
  const chart = () => renderer!.root.findByProps({ 'aria-label': 'Weekly cost' });
  const items = () => chart().findAll((n) => n.type === 'li');
  const barHeights = () => chart().findAll((n) => n.type === 'span').map((n) => (n.props.style as { height: string }).height);

  test('bars run oldest to newest with the current period last', () => {
    const li = items();
    expect(li.map((n) => labelOf(n))).toEqual(['$0.50' + '12 Oct', '$0.01' + '19 Oct']);
    expect(li[1].props['aria-current']).toBe('true');
    expect(li[0].props['aria-current']).toBeUndefined();
  });

  test('bar heights scale to the largest period', () => {
    expect(barHeights()).toEqual(['100%', '2.4%']);
  });

  test('an all-zero range still draws a 1% stub', async () => {
    COST_WEEK_OVERRIDE = [cost('2026-10-19', 0), cost('2026-10-12', 0)];
    await act(async () => {
      renderer!.unmount();
      renderer = create(<StatsPage />);
    });
    await flush();
    expect(barHeights()).toEqual(['1%', '1%']);
    COST_WEEK_OVERRIDE = null;
  });

  test('month view labels bars by month', async () => {
    await act(async () => {
      buttonWith('Month').props.onClick();
    });
    await flush();
    const month = renderer!.root.findByProps({ 'aria-label': 'Monthly cost' });
    expect(labelOf(month)).toContain('Oct');
  });
});
