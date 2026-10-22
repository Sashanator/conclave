import { afterAll, afterEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  event: { ...await import('@tauri-apps/api/event') },
  sonner: { ...await import('sonner') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('sonner', () => originals.sonner);
});

type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();
const listen = mock(async (event: string, handler: Handler) => {
  handlers.set(event, handler);
  return () => { if (handlers.get(event) === handler) handlers.delete(event); };
});
const toast = { success: mock(() => {}), error: mock((_what: string, _opts?: unknown) => {}), info: mock(() => {}), warning: mock(() => {}) };

mock.module('@tauri-apps/api/event', () => ({ listen }));
mock.module('sonner', () => ({ toast }));
const { useModalState } = await import('../../src/hooks/useModalState');

function Probe() {
  useModalState();
  return null;
}

let renderer: ReactTestRenderer | undefined;
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  handlers.clear();
  toast.error.mockClear();
});

describe('recording-error event', () => {
  test('a recording error from Rust shows a toast with its message', async () => {
    await act(async () => { renderer = create(<Probe />); });
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
    expect(handlers.has('recording-error')).toBe(true);

    const message = 'Audio stream disconnected after extended operation. Please restart recording.';
    await act(async () => { handlers.get('recording-error')!({ payload: message }); });
    expect(toast.error).toHaveBeenCalledWith(message, expect.anything());
  });
});
