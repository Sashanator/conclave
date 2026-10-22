import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  path: { ...await import('@tauri-apps/api/path') },
  sonner: { ...await import('sonner') },
  tooltip: { ...await import('../../src/components/ui/tooltip') },
  recordingState: { ...await import('../../src/contexts/RecordingStateContext') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('@tauri-apps/api/path', () => originals.path);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/components/ui/tooltip', () => originals.tooltip);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.recordingState);
});

type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();
const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;

let stopInvoke: () => Promise<unknown> = async () => undefined;
const invoke = mock(async (command: string): Promise<unknown> => {
  if (command === 'is_recording') return false;
  if (command === 'stop_recording') return stopInvoke();
  throw new Error(`Unexpected command: ${command}`);
});
// Set to hold listen() until the test releases it, as a slow Tauri registration can be
let gate: Promise<void> | null = null;
const listen = mock(async (event: string, handler: Handler) => {
  if (gate) await gate;
  handlers.set(event, handler);
  return () => { if (handlers.get(event) === handler) handlers.delete(event); };
});
const toast = { success: mock(() => {}), error: mock((_what: string, _opts?: unknown) => {}), info: mock(() => {}), warning: mock(() => {}) };

mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('@tauri-apps/api/event', () => ({ listen }));
mock.module('@tauri-apps/api/path', () => ({ appDataDir: async () => '/data' }));
mock.module('sonner', () => ({ toast }));
mock.module('../../src/components/ui/tooltip', () => ({ Tooltip: passThrough, TooltipTrigger: passThrough, TooltipContent: passThrough, TooltipProvider: passThrough }));
mock.module('../../src/contexts/RecordingStateContext', () => ({
  ...originals.recordingState,
  useRecordingState: () => ({ isPaused: false, isStartingRecording: false }),
}));
const { RecordingControls } = await import('../../src/components/RecordingControls');

let renderer: ReactTestRenderer | undefined;
let onRecordingStop: ReturnType<typeof mock>;
let onTranscriptionError: ReturnType<typeof mock>;
let onStopFailed: ReturnType<typeof mock>;

// New arrow functions on every call, as app/page.tsx passes them on every render
function props() {
  return {
    isRecording: true,
    barHeights: [] as string[],
    onRecordingStop: (callApi?: boolean) => onRecordingStop(callApi),
    onRecordingStart: async () => {},
    onTranscriptReceived: () => {},
    onTranscriptionError: (message: string) => onTranscriptionError(message),
    onStopFailed: () => onStopFailed(),
    isRecordingDisabled: false,
    isParentProcessing: false,
  };
}
async function flush() {
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
}
async function fire(event: string, payload: unknown) {
  await act(async () => { handlers.get(event)!({ payload }); });
  await flush();
}
function stopButton(): ReactTestInstance {
  return renderer!.root.findAll((n) => n.type === 'button' && String(n.props.className).includes('bg-red-500 hover:bg-red-600'))[0];
}

beforeEach(() => {
  handlers.clear();
  gate = null;
  listen.mockClear();
  onRecordingStop = mock((_callApi?: boolean) => {});
  onTranscriptionError = mock((_message: string) => {});
  onStopFailed = mock(() => {});
  stopInvoke = async () => undefined;
  invoke.mockClear();
  toast.error.mockClear();
});
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
});

describe('transcription errors while recording', () => {
  test('an active transcription error does not stop the recording', async () => {
    await act(async () => { renderer = create(<RecordingControls {...props()} />); });
    await flush();
    await fire('transcription-error', { error: 'x', userMessage: 'Transcription failed', actionable: false, phase: 'active' });
    expect(onRecordingStop).not.toHaveBeenCalled();
  });

  test('a legacy transcript error does not stop the recording but still reaches the error handler', async () => {
    await act(async () => { renderer = create(<RecordingControls {...props()} />); });
    await flush();
    await fire('transcript-error', 'Whisper crashed');
    expect(onRecordingStop).not.toHaveBeenCalled();
    expect(onTranscriptionError).toHaveBeenCalledWith('Whisper crashed');
  });
});

describe('stop failures', () => {
  test('a failed stop shows an error without taking the stop gate, and a second Stop works', async () => {
    await act(async () => { renderer = create(<RecordingControls {...props()} />); });
    await flush();

    stopInvoke = async () => { throw new Error('disk full'); };
    await act(async () => { stopButton().props.onClick(); });
    await flush();
    expect(onRecordingStop).not.toHaveBeenCalled();
    expect(toast.error).toHaveBeenCalledWith('Could not stop recording', expect.anything());
    expect(onStopFailed).toHaveBeenCalledTimes(1);

    stopInvoke = async () => undefined;
    await act(async () => { stopButton().props.onClick(); });
    await flush();
    expect(invoke.mock.calls.filter(([command]) => command === 'stop_recording')).toHaveLength(2);
    expect(onRecordingStop).toHaveBeenCalledWith(true);
  });
});

describe('recording listeners', () => {
  test('re-rendering with new callbacks does not register the listeners again', async () => {
    await act(async () => { renderer = create(<RecordingControls {...props()} />); });
    await flush();
    const registered = listen.mock.calls.length;
    expect(registered).toBeGreaterThan(0);

    await act(async () => { renderer!.update(<RecordingControls {...props()} />); });
    await flush();
    expect(listen.mock.calls.length).toBe(registered);
  });

  test('listeners that resolve after unmount are removed', async () => {
    let release!: () => void;
    gate = new Promise<void>((resolve) => { release = resolve; });
    await act(async () => { renderer = create(<RecordingControls {...props()} />); });
    await act(async () => renderer!.unmount());
    renderer = undefined;
    release();
    await flush();
    expect(handlers.size).toBe(0);
  });
});
