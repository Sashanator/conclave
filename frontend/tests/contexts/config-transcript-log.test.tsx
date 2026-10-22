import { afterAll, afterEach, beforeEach, describe, expect, mock, spyOn, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  configService: { ...await import('../../src/services/configService') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('../../src/services/configService', () => originals.configService);
});

const SECRET = 'transcription-key-secret';
const MODEL_SECRET = 'model-config-key-secret';
const invoke = mock(async (_command: string): Promise<unknown> => undefined);
type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('@tauri-apps/api/event', () => ({
  listen: async (event: string, handler: Handler) => {
    handlers.set(event, handler);
    return () => { handlers.delete(event); };
  },
}));
// Every service read resolves to nothing, except the saved transcript config that carries a key.
mock.module('../../src/services/configService', () => ({
  configService: new Proxy({}, {
    get: (_target, name) => async () => (name === 'getTranscriptConfig'
      ? { provider: 'localWhisper', model: 'large-v3', apiKey: SECRET }
      : null),
  }),
}));
const { ConfigProvider } = await import('../../src/contexts/ConfigContext');

let renderer: ReactTestRenderer | undefined;
let spies: Array<{ mockRestore: () => void }> = [];
let logs: unknown[][];
beforeEach(() => {
  logs = [];
  spies = (['log', 'error', 'warn', 'info'] as const).map((level) =>
    spyOn(console, level).mockImplementation((...args: unknown[]) => { logs.push(args); }));
});
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  spies.forEach((spy) => spy.mockRestore());
  handlers.clear();
});

describe('loading the saved transcript config', () => {
  test('the transcription API key is not written to the console', async () => {
    await act(async () => { renderer = create(<ConfigProvider><div /></ConfigProvider>); });
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
    expect(logs.some((args) => args.some((arg) => String(arg).includes('Loaded saved transcript config')))).toBe(true);
    expect(JSON.stringify(logs)).not.toContain(SECRET);
  });
});

describe('model-config-updated events', () => {
  test('the event payload, which carries the API key, is not logged whole', async () => {
    await act(async () => { renderer = create(<ConfigProvider><div /></ConfigProvider>); });
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
    await act(async () => {
      handlers.get('model-config-updated')!({ payload: { provider: 'claude', model: 'sonnet', apiKey: MODEL_SECRET } });
    });
    expect(logs.some((args) => args.some((arg) => String(arg).includes('model-config-updated event')))).toBe(true);
    expect(JSON.stringify(logs)).not.toContain(MODEL_SECRET);
  });
});
