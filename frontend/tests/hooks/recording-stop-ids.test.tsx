import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { UseRecordingStopReturn } from '../../src/hooks/useRecordingStop';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  nav: { ...await import('next/navigation') },
  sonner: { ...await import('sonner') },
  transcript: { ...await import('../../src/contexts/TranscriptContext') },
  sidebar: { ...await import('../../src/components/Sidebar/SidebarProvider') },
  state: { ...await import('../../src/contexts/RecordingStateContext') },
  storage: { ...await import('../../src/services/storageService') },
  transcriptService: { ...await import('../../src/services/transcriptService') },
  prefs: { ...await import('../../src/lib/summary-language-preferences') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('next/navigation', () => originals.nav);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/contexts/TranscriptContext', () => originals.transcript);
  mock.module('../../src/components/Sidebar/SidebarProvider', () => originals.sidebar);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.state);
  mock.module('../../src/services/storageService', () => originals.storage);
  mock.module('../../src/services/transcriptService', () => originals.transcriptService);
  mock.module('../../src/lib/summary-language-preferences', () => originals.prefs);
});

const transcriptsRef: { current: Array<{ text: string }> } = { current: [{ text: 'hi' }] };
const notesRef: { current: unknown[] } = { current: [] };
let onStatusPoll: () => void = () => {};
type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();
mock.module('@tauri-apps/api/event', () => ({
  listen: mock(async (event: string, handler: Handler) => { handlers.set(event, handler); return () => {}; }),
}));
const invoke = mock(async (_c: string, _a?: unknown) => undefined);
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('next/navigation', () => ({ useRouter: () => ({ push: () => {} }) }));
const toast = {
  success: mock((..._a: unknown[]) => {}), error: mock((..._a: unknown[]) => {}),
  warning: mock((..._a: unknown[]) => {}), info: mock((..._a: unknown[]) => {}),
};
mock.module('sonner', () => ({ toast }));
mock.module('../../src/contexts/TranscriptContext', () => ({
  useTranscripts: () => ({
    transcriptsRef, notesRef, flushBuffer() {}, clearTranscripts() {},
    meetingTitle: 'Standup', titleSource: 'default', calendarLink: null, markMeetingAsSaved: async () => {}, currentMeetingId: null,
  }),
}));
mock.module('../../src/components/Sidebar/SidebarProvider', () => ({
  useSidebar: () => ({ refetchMeetings: async () => {}, setCurrentMeeting() {}, setMeetings() {}, meetings: [], setIsMeetingActive() {} }),
}));
mock.module('../../src/contexts/RecordingStateContext', () => ({
  ...originals.state,
  useRecordingState: () => ({ status: 'idle', setStatus() {}, isStopping: false, isProcessing: false, isSaving: false }),
}));
let saveResult: Record<string, unknown> = { meeting_id: 'saved-1' };
const saveMeeting = mock(async (..._a: unknown[]) => saveResult);
mock.module('../../src/services/storageService', () => ({
  storageService: { saveMeeting, getMeeting: async () => ({ title: 'Standup' }) },
}));
mock.module('../../src/services/transcriptService', () => ({
  transcriptService: { getTranscriptionStatus: async () => { onStatusPoll(); return { is_processing: false, chunks_in_queue: 0, last_activity_ms: 0 }; } },
}));
mock.module('../../src/lib/summary-language-preferences', () => ({
  applyPinnedSummaryLanguageToMeeting: async () => true, detectAndCacheSummaryLanguage: async () => {},
}));
const { useRecordingStop } = await import('../../src/hooks/useRecordingStop');

let stop: UseRecordingStopReturn;
let renderer: ReactTestRenderer | undefined;
function View() { stop = useRecordingStop(() => {}, () => {}); return null; }
async function mount() {
  await act(async () => { renderer = create(<View />); });
  await act(async () => { await new Promise((r) => setTimeout(r, 0)); });
}
const realSetTimeout = globalThis.setTimeout;
async function stopAndSave() {
  // The hook waits seconds for late lines; the test does not.
  globalThis.setTimeout = ((fn: () => void, ms?: number) => realSetTimeout(fn, ms && ms >= 500 ? 0 : ms)) as typeof setTimeout;
  try { await act(async () => { await stop.handleRecordingStop(true); }); } finally { globalThis.setTimeout = realSetTimeout; }
}
const emitStopped = (payload: Record<string, unknown>) => act(async () => { handlers.get('recording-stopped')!({ payload }); await new Promise((r) => setTimeout(r, 0)); });

beforeEach(() => {
  saveMeeting.mockClear(); invoke.mockClear();
  transcriptsRef.current = [{ text: 'hi' }]; onStatusPoll = () => {};
  for (const t of Object.values(toast)) t.mockClear();
  saveResult = { meeting_id: 'saved-1' };
  sessionStorage.clear();
});
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined; handlers.clear();
});

describe('recording-stopped listener and the save', () => {
  test('stores the routed meeting id, and the save receives it', async () => {
    await mount();
    await emitStopped({ message: 'x', meeting_id: 'routed-1', meeting_name: 'Standup' });
    expect(sessionStorage.getItem('last_recording_meeting_id')).toBe('routed-1');
    await stopAndSave();
    expect(saveMeeting.mock.calls[0][7]).toBe('routed-1');
    expect(sessionStorage.getItem('last_recording_meeting_id')).toBeNull();
  });

  test('a stop without an id clears a stale one', async () => {
    sessionStorage.setItem('last_recording_meeting_id', 'stale');
    await mount();
    await emitStopped({ message: 'x', meeting_id: null });
    expect(sessionStorage.getItem('last_recording_meeting_id')).toBeNull();
    await stopAndSave();
    expect(saveMeeting.mock.calls[0][7]).toBeNull();
  });
});

describe('save outcome toasts', () => {
  test('a fallback shows "Saved as a new meeting"', async () => {
    saveResult = { meeting_id: 'n1', fallback: true };
    await mount(); await stopAndSave();
    expect(toast.warning).toHaveBeenCalledWith('Saved as a new meeting', expect.anything());
  });

  test('a release_error shows the left-behind toast', async () => {
    saveResult = { meeting_id: 'n1', release_error: 'locked' };
    await mount(); await stopAndSave();
    expect(toast.error).toHaveBeenCalledWith('An empty meeting was left behind', expect.anything());
  });

  test('a continue error shows its own toast and no "Saved as a new meeting"', async () => {
    saveResult = { meeting_id: 'n1', fallback: true, continue_error: 'gone' };
    await mount(); await stopAndSave();
    expect(toast.error).toHaveBeenCalledWith('Could not add this recording to the earlier meeting', expect.anything());
    expect(toast.warning).not.toHaveBeenCalledWith('Saved as a new meeting', expect.anything());
  });
});

describe('a new recording during the previous save', () => {
  test('the save still gets the first recording\'s id and lines', async () => {
    await mount();
    await emitStopped({ message: 'x', meeting_id: 'meeting-A', meeting_name: 'A' });
    transcriptsRef.current = [{ text: 'A line 1' }, { text: 'A line 2' }];
    // Recording B starts while A is still waiting for its transcription: what a start does to shared state.
    onStatusPoll = () => {
      transcriptsRef.current = [];
      sessionStorage.removeItem('last_recording_meeting_id');
      handlers.get('recording-started')?.({ payload: { meetingId: 'meeting-B' } });
    };
    await stopAndSave();
    expect(saveMeeting).toHaveBeenCalledTimes(1);
    const args = saveMeeting.mock.calls[0];
    expect((args[1] as Array<{ text: string }>).map((t) => t.text)).toEqual(['A line 1', 'A line 2']);
    expect(args[7]).toBe('meeting-A');
  });

  test('the saving target is released when the save ends', async () => {
    await mount();
    await emitStopped({ message: 'x', meeting_id: 'meeting-A' });
    await stopAndSave();
    expect(invoke).toHaveBeenCalledWith('api_finish_saving_target', { meetingId: 'meeting-A' });
  });
});

describe('a failed continue', () => {
  test('with a routed meeting it says the recording stayed in its own meeting', async () => {
    saveResult = { meeting_id: 'meeting-A', continue_error: 'gone' };
    await mount();
    await emitStopped({ message: 'x', meeting_id: 'meeting-A' });
    await stopAndSave();
    expect(toast.error).toHaveBeenCalledWith('Could not add this recording to the earlier meeting',
      { description: 'gone It was saved into the meeting it started in instead.' });
  });

  test('without a routed meeting it says a new meeting', async () => {
    saveResult = { meeting_id: 'n1', continue_error: 'gone' };
    await mount();
    await stopAndSave();
    expect(toast.error).toHaveBeenCalledWith('Could not add this recording to the earlier meeting',
      { description: 'gone It was saved as a new meeting instead.' });
  });
});
