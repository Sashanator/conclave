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

const invoke = mock(async (command: string): Promise<unknown> => {
  if (command === 'extraction_status') return status;
  if (command === 'extraction_retry') return null;
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

async function mount() {
  renderer?.unmount();
  renderer = null;
  await act(async () => {
    renderer = create(<ExtractionStatusLine meetingId="m1" />);
  });
  await flush();
}

beforeEach(() => {
  invoke.mockClear();
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
});
