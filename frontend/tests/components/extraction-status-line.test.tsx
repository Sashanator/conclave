import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
});

let status: string | null;
// When set, a retry leaves the extraction pending, as when one is already in flight
let retryLeavesPending = false;

const invoke = mock(async (command: string): Promise<unknown> => {
  if (command === 'extraction_status') return status;
  if (command === 'extraction_retry') {
    if (retryLeavesPending) status = 'pending';
    return null;
  }
  throw new Error(`Unexpected command: ${command}`);
});

mock.module('@tauri-apps/api/core', () => ({ invoke }));

const { ExtractionStatusLine } = await import('../../src/components/MeetingDetails/ExtractionStatusLine');

let renderer: ReactTestRenderer | null = null;

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

function textOf(node: ReactTestInstance): string {
  return node.children.map((c) => (typeof c === 'string' ? c : textOf(c))).join('');
}

async function mount(recheckMs?: number) {
  renderer?.unmount();
  renderer = null;
  await act(async () => {
    renderer = create(<ExtractionStatusLine meetingId="m1" recheckMs={recheckMs} />);
  });
  await flush();
}

async function wait(ms: number) {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, ms));
  });
}

beforeEach(() => {
  invoke.mockClear();
  retryLeavesPending = false;
});

describe('ExtractionStatusLine', () => {
  test('shows nothing when extraction is done or never ran', async () => {
    status = 'done';
    await mount();
    expect(renderer!.toJSON()).toBeNull();
    status = null;
    await mount();
    expect(renderer!.toJSON()).toBeNull();
  });

  test('shows Extracting tasks while pending', async () => {
    status = 'pending';
    await mount();
    expect(textOf(renderer!.root.findAll((n) => n.type === 'p')[0])).toBe('Extracting tasks…');
  });

  test('a failed extraction shows Retry, which calls extraction_retry', async () => {
    status = 'failed';
    await mount();
    const retry = renderer!.root.findAll((n) => n.type === 'button' && textOf(n) === 'Retry')[0];
    expect(renderer!.root.findAll((n) => textOf(n) === 'Task extraction failed').length).toBeGreaterThan(0);
    await act(async () => {
      await retry.props.onClick();
    });
    expect(invoke).toHaveBeenCalledWith('extraction_retry', { meetingId: 'm1' });
  });

  test('a retry that leaves the status pending keeps polling until the final status', async () => {
    status = 'failed';
    retryLeavesPending = true;
    await mount(5);
    const retry = renderer!.root.findAll((n) => n.type === 'button' && textOf(n) === 'Retry')[0];
    await act(async () => {
      await retry.props.onClick();
    });
    await flush();
    expect(textOf(renderer!.root.findAll((n) => n.type === 'p')[0])).toBe('Extracting tasks…');

    // The extraction finishes after the retry returned
    status = 'done';
    await wait(40);
    expect(renderer!.toJSON()).toBeNull();
    expect(invoke.mock.calls.filter(([c]) => c === 'extraction_status').length).toBeGreaterThan(2);
  });
});
