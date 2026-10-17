import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { TranscriptUpdate } from '../../src/types';

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

const recording = { isRecording: false };
mock.module('../../src/contexts/RecordingStateContext', () => ({ useRecordingState: () => recording }));

const listeners: Array<(update: TranscriptUpdate) => void> = [];
const getTranscriptHistory = mock(async (): Promise<unknown[]> => []);
mock.module('../../src/services/transcriptService', () => ({
  transcriptService: {
    onTranscriptUpdate: mock(async (cb: (update: TranscriptUpdate) => void) => { listeners.push(cb); return () => {}; }),
    getTranscriptHistory,
  },
}));
mock.module('../../src/services/recordingService', () => ({
  recordingService: {
    onRecordingStarted: mock(async () => () => {}),
    onRecordingStopped: mock(async () => () => {}),
    getRecordingMeetingName: mock(async () => null),
  },
}));
mock.module('../../src/services/indexedDBService', () => ({
  indexedDBService: {
    init: mock(async () => {}),
    getMeetingMetadata: mock(async () => null),
    patchMeetingMetadata: mock(async () => {}),
    saveTranscript: mock(async () => {}),
    saveMeetingMetadata: mock(async () => {}),
  },
}));
mock.module('sonner', () => ({ toast: { success: mock(() => {}), error: mock(() => {}), warning: mock(() => {}), info: mock(() => {}) } }));

const { TranscriptProvider, useTranscripts } = await import('../../src/contexts/TranscriptContext');

let api: ReturnType<typeof useTranscripts>;
function Probe() {
  api = useTranscripts();
  return null;
}
function tree(children?: ReactNode) {
  return <TranscriptProvider><Probe />{children}</TranscriptProvider>;
}

let renderer: ReactTestRenderer | undefined;
async function settle() {
  // The live buffer flushes on a 10 ms timer
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 30)); });
}

const update: TranscriptUpdate = {
  text: 'Hi.', timestamp: '10:00:01', source: 'mic', sequence_id: 1, chunk_start_time: 1,
  is_partial: false, confidence: 0.9, audio_start_time: 1, audio_end_time: 2, duration: 1,
  speaker_idx: 0, sentences: [[1, 2, 'Hi.']],
};

beforeEach(() => {
  recording.isRecording = false;
  listeners.length = 0;
  getTranscriptHistory.mockClear();
  getTranscriptHistory.mockImplementation(async () => []);
});
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
});

describe('transcript speaker fields', () => {
  test('a live line keeps speaker_idx and sentences', async () => {
    await act(async () => { renderer = create(tree()); });
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
    await act(async () => { listeners.at(-1)!(update); });
    await settle();
    expect(api.transcripts[0]).toMatchObject({ text: 'Hi.', speaker_idx: 0, sentences: [[1, 2, 'Hi.']] });
  });

  test('the history rebuilt after a reload keeps speaker_idx and sentences', async () => {
    recording.isRecording = true;
    getTranscriptHistory.mockImplementation(async () => [{
      id: 'seg_1', text: 'Hi.', display_time: '10:00:01', sequence_id: 1, confidence: 0.9,
      audio_start_time: 1, audio_end_time: 2, duration: 1, speaker_idx: 0, sentences: [[1, 2, 'Hi.']],
    }]);
    await act(async () => { renderer = create(tree()); });
    await settle();
    expect(getTranscriptHistory).toHaveBeenCalled();
    expect(api.transcripts[0]).toMatchObject({ text: 'Hi.', speaker_idx: 0, sentences: [[1, 2, 'Hi.']] });
  });
});
