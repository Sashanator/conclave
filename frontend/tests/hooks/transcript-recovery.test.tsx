import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { UseTranscriptRecoveryReturn } from '../../src/hooks/useTranscriptRecovery';

const originalCore = { ...await import('@tauri-apps/api/core') };
const originalSonner = { ...await import('sonner') };

// What `recover_audio_from_checkpoints` does in a given test; set per test.
let recoverAudio: () => Promise<unknown> = async () => ({ status: 'success', chunk_count: 2, estimated_duration_seconds: 60, message: 'ok' });
const invoke = mock(async (command: string) => {
  if (command === 'recover_audio_from_checkpoints') return recoverAudio();
  return undefined;
});
mock.module('@tauri-apps/api/core', () => ({ ...originalCore, invoke }));
mock.module('sonner', () => ({ toast: { success() {}, warning() {}, error() {} } }));
// The lines the checkpoint holds; set per test.
let storedTranscripts: Array<Record<string, unknown>> = [
  { id: 't1', text: 'hello', timestamp: '2026-10-16T10:00:00Z', sequenceId: 1, confidence: 0.9 },
];
const savedMeetings: Array<{ transcripts: Array<Record<string, unknown>> }> = [];
mock.module('../../src/services/indexedDBService', () => ({
  indexedDBService: {
    getMeetingMetadata: async () => ({ meetingId: 'meeting-local-1', title: 'Standup', folderPath: '/recordings/Standup' }),
    getTranscripts: async () => storedTranscripts,
    markMeetingSaved: async () => {},
  },
}));
mock.module('../../src/services/storageService', () => ({
  storageService: {
    saveMeeting: async (_title: string, transcripts: Array<Record<string, unknown>>) => {
      savedMeetings.push({ transcripts });
      return { meeting_id: 'meeting-1' };
    },
  },
}));
mock.module('../../src/lib/summary-language-preferences', () => ({ applyPinnedSummaryLanguageToMeeting: async () => {} }));

const { useTranscriptRecovery } = await import('../../src/hooks/useTranscriptRecovery');

afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originalCore);
  mock.module('sonner', () => originalSonner);
});

let recovery: UseTranscriptRecoveryReturn;
let renderer: ReactTestRenderer | undefined;
function View() {
  recovery = useTranscriptRecovery();
  return null;
}

const realConsoleError = console.error;
beforeEach(() => {
  invoke.mockClear();
  console.error = () => {}; // the hook logs a recovery failure by design
});
afterEach(async () => {
  console.error = realConsoleError;
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
});

async function recoverAndCountCleanups(): Promise<number> {
  await act(async () => {
    renderer = create(<View />);
  });
  await act(async () => {
    await recovery.recoverMeeting('meeting-local-1');
  });
  return invoke.mock.calls.filter(([command]) => command === 'cleanup_checkpoints').length;
}

describe('recoverMeeting checkpoint cleanup', () => {
  test('a merged recovery cleans up the checkpoints', async () => {
    recoverAudio = async () => ({ status: 'success', chunk_count: 2, estimated_duration_seconds: 60, message: 'ok' });
    expect(await recoverAndCountCleanups()).toBe(1);
  });

  test('a failed recovery keeps the checkpoints: they are the only copy', async () => {
    recoverAudio = async () => ({ status: 'failed', chunk_count: 2, estimated_duration_seconds: 60, message: 'FFmpeg failed' });
    expect(await recoverAndCountCleanups()).toBe(0);
  });

  test('a recovery that throws keeps the checkpoints too', async () => {
    recoverAudio = async () => { throw new Error('boom'); };
    expect(await recoverAndCountCleanups()).toBe(0);
  });

  test('a partial recovery keeps the checkpoints', async () => {
    recoverAudio = async () => ({ status: 'partial', chunk_count: 2, estimated_duration_seconds: 60, message: 'partial' });
    expect(await recoverAndCountCleanups()).toBe(0);
  });
});

describe('recovery keeps the speaker fields', () => {
  test('a stored line keeps speaker_idx and sentences; a line without them gets no such keys', async () => {
    storedTranscripts = [
      { id: 't1', text: 'Hi.', timestamp: '2026-10-16T10:00:00Z', sequenceId: 1, confidence: 0.9, speaker_idx: 0, sentences: [[1, 2, 'Hi.']] },
      { id: 't2', text: 'Bye.', timestamp: '2026-10-16T10:00:05Z', sequenceId: 2, confidence: 0.9 },
    ];
    savedMeetings.length = 0;
    recoverAudio = async () => ({ status: 'success', chunk_count: 2, estimated_duration_seconds: 60, message: 'ok' });
    await recoverAndCountCleanups();
    const [first, second] = savedMeetings.at(-1)!.transcripts;
    expect(first).toMatchObject({ speaker_idx: 0, sentences: [[1, 2, 'Hi.']] });
    expect(second).not.toHaveProperty('speaker_idx');
    expect(second).not.toHaveProperty('sentences');
    storedTranscripts = [{ id: 't1', text: 'hello', timestamp: '2026-10-16T10:00:00Z', sequenceId: 1, confidence: 0.9 }];
  });
});
