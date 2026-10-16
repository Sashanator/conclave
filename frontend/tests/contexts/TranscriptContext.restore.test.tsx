import { afterAll, afterEach, beforeEach, describe, expect, mock, spyOn, test } from 'bun:test';
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

// Reload during a recording: the restore runs with isRecording already true.
const recording = { isRecording: true };
mock.module('../../src/contexts/RecordingStateContext', () => ({ useRecordingState: () => recording }));

const listeners: Array<(update: TranscriptUpdate) => void> = [];
mock.module('../../src/services/transcriptService', () => ({
  transcriptService: {
    onTranscriptUpdate: mock(async (cb: (update: TranscriptUpdate) => void) => { listeners.push(cb); return () => {}; }),
    getTranscriptHistory: mock(async () => []),
  },
}));
mock.module('../../src/services/recordingService', () => ({
  recordingService: {
    onRecordingStarted: mock(async () => () => {}),
    onRecordingStopped: mock(async () => () => {}),
    getRecordingMeetingName: mock(async () => null),
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

const { TranscriptProvider, useTranscripts } = await import('../../src/contexts/TranscriptContext');
const MEETING = 'meeting-restore-1';

let api: ReturnType<typeof useTranscripts>;
function Probe() {
  api = useTranscripts();
  return null;
}
function tree(children?: ReactNode) {
  return <TranscriptProvider><Probe />{children}</TranscriptProvider>;
}

let renderer: ReactTestRenderer | undefined;
async function flush() {
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
}
const update: TranscriptUpdate = {
  text: 'said after the reload', timestamp: '10:00:01', source: 'mic', sequence_id: 7, chunk_start_time: 0,
  is_partial: false, confidence: 0.9, audio_start_time: 12, audio_end_time: 14, duration: 2,
};

beforeEach(() => {
  sessionStorage.setItem('indexeddb_current_meeting_id', MEETING);
  recording.isRecording = true;
  listeners.length = 0;
  indexedDB.getMeetingMetadata.mockClear();
  indexedDB.getMeetingMetadata.mockImplementation(async () => ({ notes: [], marks: {} }));
  indexedDB.patchMeetingMetadata.mockClear();
  indexedDB.saveTranscript.mockClear();
});
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  sessionStorage.removeItem('indexeddb_current_meeting_id');
});

describe('TranscriptContext restore after a reload during a recording', () => {
  test('lines that arrive after the restore are written to the meeting record', async () => {
    await act(async () => { renderer = create(tree()); });
    await flush();
    expect(indexedDB.getMeetingMetadata).toHaveBeenCalledWith(MEETING);
    await act(async () => { listeners.at(-1)!(update); });
    expect(indexedDB.saveTranscript).toHaveBeenCalledWith(MEETING, update);
  });

  test('a failed restore does not mark the record ready, and a later recording change retries it', async () => {
    const logged = spyOn(console, 'error').mockImplementation(() => {});
    indexedDB.getMeetingMetadata.mockImplementationOnce(async () => { throw new Error('db busy'); });
    await act(async () => { renderer = create(tree()); });
    await flush();
    expect(logged).toHaveBeenCalledWith(expect.stringContaining('Could not load notes and marks'), expect.any(Error));
    await act(async () => { api.addNote('kept?', null); });
    await flush();
    expect(indexedDB.patchMeetingMetadata).not.toHaveBeenCalled();
    logged.mockRestore();

    recording.isRecording = false;
    await act(async () => { renderer!.update(tree()); });
    recording.isRecording = true;
    await act(async () => { renderer!.update(tree()); });
    await flush();
    expect(indexedDB.getMeetingMetadata).toHaveBeenCalledTimes(2);
    expect(indexedDB.patchMeetingMetadata).toHaveBeenCalledWith(MEETING, expect.objectContaining({
      notes: [expect.objectContaining({ text: 'kept?' })],
    }));
  });
});
