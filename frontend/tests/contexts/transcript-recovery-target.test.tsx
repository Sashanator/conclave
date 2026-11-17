import { afterAll, afterEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  recordingState: { ...await import('../../src/contexts/RecordingStateContext') },
  transcriptService: { ...await import('../../src/services/transcriptService') },
  recordingService: { ...await import('../../src/services/recordingService') },
  indexedDBService: { ...await import('../../src/services/indexedDBService') },
  sonner: { ...await import('sonner') },
};
afterAll(() => {
  mock.module('../../src/contexts/RecordingStateContext', () => originals.recordingState);
  mock.module('../../src/services/transcriptService', () => originals.transcriptService);
  mock.module('../../src/services/recordingService', () => originals.recordingService);
  mock.module('../../src/services/indexedDBService', () => originals.indexedDBService);
  mock.module('sonner', () => originals.sonner);
});

// Not recording: the start listener is what writes the metadata.
const recording = { isRecording: false };
mock.module('../../src/contexts/RecordingStateContext', () => ({ useRecordingState: () => recording }));

const listeners: Array<(update: unknown) => void> = [];
mock.module('../../src/services/transcriptService', () => ({
  transcriptService: {
    onTranscriptUpdate: mock(async (cb: (update: unknown) => void) => { listeners.push(cb); return () => {}; }),
    getTranscriptHistory: mock(async () => []),
  },
}));
const startedCallbacks: Array<(p?: { meetingId?: string | null }) => Promise<void>> = [];
let meetingNameFromBackend: string | null = null;
mock.module('../../src/services/recordingService', () => ({
  recordingService: {
    onRecordingStarted: mock(async (cb: (p?: { meetingId?: string | null }) => Promise<void>) => { startedCallbacks.push(cb); return () => {}; }),
    onRecordingStopped: mock(async () => () => {}),
    getRecordingMeetingName: mock(async () => meetingNameFromBackend),
  },
}));
const indexedDB = {
  init: mock(async () => {}),
  getMeetingMetadata: mock(async (_id: string) => ({ notes: [], marks: {} })),
  patchMeetingMetadata: mock(async (_id: string, _patch: unknown) => {}),
  saveTranscript: mock(async (_id: string, _update: unknown) => {}),
  saveMeetingMetadata: mock(async (_meta: unknown) => {}),
};
mock.module('../../src/services/indexedDBService', () => ({ indexedDBService: indexedDB }));
mock.module('sonner', () => ({ toast: { success: mock(() => {}), error: mock(() => {}), warning: mock(() => {}), info: mock(() => {}) } }));

const { TranscriptProvider } = await import('../../src/contexts/TranscriptContext');


let renderer: ReactTestRenderer | undefined;
async function flush() {
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
}
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  startedCallbacks.length = 0;
  indexedDB.saveMeetingMetadata.mockClear();
  sessionStorage.clear();
});

async function startWith(payload?: { meetingId?: string | null }) {
  await act(async () => { renderer = create(<TranscriptProvider><div /></TranscriptProvider>); });
  await flush();
  await act(async () => { await startedCallbacks.at(-1)!(payload); });
}

describe('recording-started records the target meeting for crash recovery', () => {
  test('the payload meetingId becomes targetMeetingId', async () => {
    await startWith({ meetingId: 'm-t' });
    expect(indexedDB.saveMeetingMetadata).toHaveBeenCalledWith(expect.objectContaining({ targetMeetingId: 'm-t' }));
  });

  test('a null payload id gives undefined', async () => {
    await startWith({ meetingId: null });
    const saved = indexedDB.saveMeetingMetadata.mock.calls[0][0] as Record<string, unknown>;
    expect(saved.targetMeetingId).toBeUndefined();
  });
});
