import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactTestInstance, ReactTestRenderer } from 'react-test-renderer';
import { act, create } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  renderer?.unmount();
  mock.module('@tauri-apps/api/core', () => originals.core);
});

const PRICED = {
  total_usd: 0.012,
  tokens: 18400,
  by_purpose: [{ purpose: 'summary', tokens: 18400, cost_usd: 0.012 }],
};
const UNPRICED = {
  total_usd: null,
  tokens: 18400,
  by_purpose: [
    { purpose: 'summary', tokens: 12000, cost_usd: 0.012 },
    { purpose: 'extraction', tokens: 6400, cost_usd: null },
  ],
};

let meetingCost: unknown = null;

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'meeting_cost') return structuredClone(meetingCost);
  throw new Error(`Unexpected command: ${command} ${JSON.stringify(args ?? {})}`);
});

mock.module('@tauri-apps/api/core', () => ({ invoke }));

const { MeetingCostLine } = await import('../../src/components/MeetingDetails/MeetingCostLine');

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

async function mount() {
  renderer?.unmount();
  await act(async () => {
    renderer = create(<MeetingCostLine meetingId="m-1" />);
  });
  await flush();
}

beforeEach(() => {
  invoke.mockClear();
});

describe('meeting cost line', () => {
  test('a priced meeting shows the exact copy', async () => {
    meetingCost = PRICED;
    await mount();

    expect(invoke).toHaveBeenCalledWith('meeting_cost', { meetingId: 'm-1' });
    expect(labelOf(renderer!.root)).toContain('LLM cost: $0.012 · 18.4k tokens');
  });

  test('an unpriced call shows a dash for the cost', async () => {
    meetingCost = UNPRICED;
    await mount();

    expect(labelOf(renderer!.root)).toContain('LLM cost: — · 18.4k tokens');
  });

  test('a meeting with no LLM calls renders nothing', async () => {
    meetingCost = null;
    await mount();

    expect(renderer!.toJSON()).toBeNull();
  });

  test('clicking the line lists the purposes', async () => {
    meetingCost = UNPRICED;
    await mount();
    expect(labelOf(renderer!.root)).not.toContain('extraction · 6.4k tokens');

    const toggle = renderer!.root.findAll((n) => n.type === 'button')[0];
    await act(async () => {
      toggle.props.onClick();
    });

    expect(labelOf(renderer!.root)).toContain('extraction · 6.4k tokens · —');
  });
});
