import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactTestInstance, ReactTestRenderer } from 'react-test-renderer';
import { act, create } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  navigation: { ...await import('next/navigation') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  renderer?.unmount();
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('next/navigation', () => originals.navigation);
});

type Call = { command: string; args?: Record<string, unknown> };
let calls: Call[] = [];
// One entry per ask_meetings call that has not been answered yet.
let asks: Array<{ resolve: (value: unknown) => void; reject: (error: unknown) => void }> = [];

const invoke = mock((command: string, args?: Record<string, unknown>): Promise<unknown> => {
  calls.push({ command, args });
  if (command === 'ask_meetings') return new Promise((resolve, reject) => asks.push({ resolve, reject }));
  if (command === 'ask_cancel') return Promise.resolve();
  throw new Error(`Unexpected command: ${command}`);
});
const routerPush = mock((_href: string) => {});

mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('next/navigation', () => ({ useRouter: () => ({ push: routerPush }) }));

const { default: AskPage } = await import('../../src/app/ask/page');

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

function askCalls(): Record<string, unknown>[] {
  return calls.filter((c) => c.command === 'ask_meetings').map((c) => c.args!);
}

function textarea(): ReactTestInstance {
  return renderer!.root.findByType('textarea');
}

async function send(text: string) {
  await act(async () => {
    textarea().props.onChange({ target: { value: text } });
  });
  await act(async () => {
    textarea().props.onKeyDown({ key: 'Enter', shiftKey: false, nativeEvent: { isComposing: false }, preventDefault() {} });
  });
  await flush();
}

async function resolveAsk(answer: { text: string; citations: unknown[] }) {
  const pending = asks.shift()!;
  await act(async () => {
    pending.resolve(answer);
  });
  await flush();
}

const transcriptCitation = {
  id: 'E3',
  meetingId: 'm-1',
  meetingTitle: 'Standup',
  date: '2026-10-20',
  transcriptId: 'tx-9',
  audioStart: 724,
  source: 'transcript',
};
const summaryCitation = {
  id: 'E5',
  meetingId: 'm-2',
  meetingTitle: 'Review',
  date: '2026-10-19',
  transcriptId: null,
  audioStart: null,
  source: 'summary',
};

beforeEach(async () => {
  await act(async () => {
    renderer?.unmount();
  });
  calls = [];
  asks = [];
  routerPush.mockClear();
  await act(async () => {
    renderer = create(<AskPage />);
  });
  await flush();
});

describe('Ask page', () => {
  test('sending calls ask_meetings with the history of answered turns', async () => {
    await send('First?');
    expect(askCalls()[0]).toEqual({ requestId: expect.any(Number), question: 'First?', history: [] });

    await resolveAsk({ text: 'One [E1].', citations: [] });
    await send('Second?');

    expect(askCalls()[1]).toEqual({
      requestId: expect.any(Number),
      question: 'Second?',
      history: [
        { role: 'user', text: 'First?' },
        { role: 'assistant', text: 'One [E1].' },
      ],
    });
    expect(askCalls()[1].requestId as number).toBeGreaterThan(askCalls()[0].requestId as number);
  });

  test('a citation renders as a link with the right href and label', async () => {
    await send('Q');
    await resolveAsk({ text: 'Ship it [E3] and see [E5].', citations: [transcriptCitation, summaryCitation] });

    const links = renderer!.root.findAll((n) => n.type === 'a');
    expect(links.map((a) => a.props.href)).toEqual([
      '/meeting-details?id=m-1&line=tx-9',
      '/meeting-details?id=m-2&tab=summary',
    ]);
    expect(pageText()).toContain('Standup · 20 Oct · 12:04');

    await act(async () => {
      links[0].props.onClick({ preventDefault() {} });
    });
    expect(routerPush).toHaveBeenCalledWith('/meeting-details?id=m-1&line=tx-9');
  });

  test('answer_rendered_as_text: markup, URLs and markdown links render literally', async () => {
    await send('Q');
    await resolveAsk({
      text: 'Bad <img src=x onerror=alert(1)> see https://evil.example and [click](https://evil.example)',
      citations: [],
    });

    expect(pageText()).toContain('<img src=x onerror=alert(1)>');
    expect(pageText()).toContain('[click](https://evil.example)');
    expect(renderer!.root.findAll((n) => n.type === 'img')).toHaveLength(0);
    expect(renderer!.root.findAll((n) => n.type === 'a')).toHaveLength(0);
  });

  test('Cancel calls ask_cancel with the pending request id', async () => {
    await send('Q');
    const requestId = askCalls()[0].requestId;

    await act(async () => {
      buttonWith('Cancel').props.onClick();
    });
    await flush();

    expect(calls.filter((c) => c.command === 'ask_cancel')).toEqual([{ command: 'ask_cancel', args: { requestId } }]);
    expect(pageText()).toContain('Cancelled');
  });

  test('New chat clears the turns and cancels a pending question', async () => {
    await send('Pending?');
    const requestId = askCalls()[0].requestId;

    await act(async () => {
      buttonWith('New chat').props.onClick();
    });
    await flush();

    expect(calls.filter((c) => c.command === 'ask_cancel')).toEqual([{ command: 'ask_cancel', args: { requestId } }]);
    expect(pageText()).not.toContain('Pending?');
  });

  test('sending is disabled while one question is pending', async () => {
    await send('Q');
    expect(textarea().props.disabled).toBe(true);
    await send('Second');
    expect(askCalls()).toHaveLength(1);
  });

  test('an error shows inline and Retry asks the same question again', async () => {
    await send('Q');
    await act(async () => {
      asks.shift()!.reject('Set a model in Settings → Knowledge');
    });
    await flush();
    expect(pageText()).toContain('Set a model in Settings → Knowledge');

    await act(async () => {
      buttonWith('Retry').props.onClick();
    });
    await flush();
    expect(askCalls()).toHaveLength(2);
    expect(askCalls()[1]).toEqual({ requestId: expect.any(Number), question: 'Q', history: [] });
  });

  test('leaving the page cancels the pending question', async () => {
    await send('Q');
    const requestId = askCalls()[0].requestId;

    await act(async () => {
      renderer!.unmount();
    });
    renderer = null;

    expect(calls.filter((c) => c.command === 'ask_cancel')).toEqual([{ command: 'ask_cancel', args: { requestId } }]);
  });
});
