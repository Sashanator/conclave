import { afterAll, afterEach, beforeEach, describe, expect, mock, spyOn, test } from 'bun:test';
import { forwardRef, type ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingSpeakers, SummaryProcessResponse, Transcript } from '../../src/types';

// Bun shares module mocks between test files; restore the application modules after this suite.
const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  config: { ...await import('../../src/contexts/ConfigContext') },
  sidebar: { ...await import('../../src/components/Sidebar/SidebarProvider') },
  motion: { ...await import('framer-motion') },
  sonner: { ...await import('sonner') },
  navigation: { ...await import('next/navigation') },
  recordingState: { ...await import('../../src/contexts/RecordingStateContext') },
  preferences: { ...await import('../../src/lib/summary-language-preferences') },
  blockNote: { ...await import('../../src/components/AISummary/BlockNoteSummaryView') },
  empty: { ...await import('../../src/components/EmptyStateSummary') },
  generator: { ...await import('../../src/components/MeetingDetails/SummaryGeneratorButtonGroup') },
  updater: { ...await import('../../src/components/MeetingDetails/SummaryUpdaterButtonGroup') },
  popover: { ...await import('../../src/components/ui/popover') },
  picker: { ...await import('../../src/components/LanguagePickerPopover') },
  recents: { ...await import('../../src/hooks/useRecentLanguages') },
  transcriptPanel: { ...await import('../../src/components/MeetingDetails/TranscriptPanel') },
  splitView: { ...await import('../../src/components/MeetingPage/FinishedTabs') },
  rail: { ...await import('../../src/components/MeetingPage/MeetingRail') },
  crumb: { ...await import('../../src/components/BackCrumb') },
  aiSidebar: { ...await import('../../src/components/AiSidebar/AiSidebarProvider') },
  audioBar: { ...await import('../../src/components/MeetingDetails/AudioPlayerBar') },
  notesPane: { ...await import('../../src/components/MeetingNotes/NotesPane') },
  meetingData: { ...await import('../../src/hooks/meeting-details/useMeetingData') },
  templates: { ...await import('../../src/hooks/meeting-details/useTemplates') },
  meetingOperations: { ...await import('../../src/hooks/meeting-details/useMeetingOperations') },
  notes: { ...await import('../../src/hooks/useMeetingNotes') },
  audio: { ...await import('../../src/hooks/useMeetingAudio') },
  desktop: { ...await import('../../src/hooks/useIsDesktop') },
  start: { ...await import('../../src/hooks/useRecordingStart') },
  modal: { ...await import('../../src/hooks/useModalState') },
  settingsModal: { ...await import('../../src/app/_components/SettingsModal') },
  header: { ...await import('../../src/components/MeetingPage/MeetingHeader') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('../../src/contexts/ConfigContext', () => originals.config);
  mock.module('../../src/components/Sidebar/SidebarProvider', () => originals.sidebar);
  mock.module('framer-motion', () => originals.motion);
  mock.module('sonner', () => originals.sonner);
  mock.module('next/navigation', () => originals.navigation);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.recordingState);
  mock.module('../../src/lib/summary-language-preferences', () => originals.preferences);
  mock.module('../../src/components/AISummary/BlockNoteSummaryView', () => originals.blockNote);
  mock.module('../../src/components/EmptyStateSummary', () => originals.empty);
  mock.module('../../src/components/MeetingDetails/SummaryGeneratorButtonGroup', () => originals.generator);
  mock.module('../../src/components/MeetingDetails/SummaryUpdaterButtonGroup', () => originals.updater);
  mock.module('../../src/components/ui/popover', () => originals.popover);
  mock.module('../../src/components/LanguagePickerPopover', () => originals.picker);
  mock.module('../../src/hooks/useRecentLanguages', () => originals.recents);
  mock.module('../../src/components/MeetingDetails/TranscriptPanel', () => originals.transcriptPanel);
  mock.module('../../src/components/MeetingPage/FinishedTabs', () => originals.splitView);
  mock.module('../../src/components/MeetingPage/MeetingRail', () => originals.rail);
  mock.module('../../src/components/BackCrumb', () => originals.crumb);
  mock.module('../../src/components/AiSidebar/AiSidebarProvider', () => originals.aiSidebar);
  mock.module('../../src/components/MeetingDetails/AudioPlayerBar', () => originals.audioBar);
  mock.module('../../src/components/MeetingNotes/NotesPane', () => originals.notesPane);
  mock.module('../../src/hooks/meeting-details/useMeetingData', () => originals.meetingData);
  mock.module('../../src/hooks/meeting-details/useTemplates', () => originals.templates);
  mock.module('../../src/hooks/meeting-details/useMeetingOperations', () => originals.meetingOperations);
  mock.module('../../src/hooks/useMeetingNotes', () => originals.notes);
  mock.module('../../src/hooks/useMeetingAudio', () => originals.audio);
  mock.module('../../src/hooks/useIsDesktop', () => originals.desktop);
  mock.module('../../src/hooks/useRecordingStart', () => originals.start);
  mock.module('../../src/hooks/useModalState', () => originals.modal);
  mock.module('../../src/app/_components/SettingsModal', () => originals.settingsModal);
  mock.module('../../src/components/MeetingPage/MeetingHeader', () => originals.header);
});

const realSetTimeout = globalThis.setTimeout;

type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();
let jobReply: MeetingSpeakers = { job: null, speakers: [] };
let transcriptRows: Partial<Transcript>[] = [];
const invoke = mock(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'speakers_get_meeting') { return jobReply; }
  if (command === 'speakers_name') return 'p1';
  if (command === 'api_get_meeting_transcripts') return { transcripts: transcriptRows, total_count: transcriptRows.length, has_more: false };
  if (command === 'api_list_meeting_rows') return [{ id: 'meeting-a', title: 'Meeting A', startedAt: 1, hasTranscript: true }];
  if (command === 'api_get_summary') return { status: 'idle', data: null };
  if (command === 'api_get_model_config') return { model: 'claude-haiku-5-5' };
  if (command === 'api_get_meeting_metadata') return { id: 'meeting-a', title: 'Meeting A', created_at: 'a', updated_at: 'b', folder_path: null };
  if (command === 'api_process_transcript') return { process_id: 'attempt-a' };
  if (command === 'api_cancel_summary') return { cancelled: true };
  return undefined;
});
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));
mock.module('@tauri-apps/api/event', () => ({
  ...originals.event,
  listen: async (event: string, handler: Handler) => {
    handlers.set(event, handler);
    return () => handlers.delete(event);
  },
}));

// The gate reads the settings answer from ConfigContext; the test flips it to simulate the lookup answering
let settings = { speakerIdActive: false, speakerSettingsLoaded: true };
mock.module('../../src/contexts/ConfigContext', () => ({
  useConfig: () => ({
    isAutoSummary: true,
    modelConfig: { provider: 'claude', model: 'claude-haiku-5-5', whisperModel: 'large-v3-turbo', ollamaEndpoint: null },
    setModelConfig: () => {},
    isModelConfigLoading: false,
    ...settings,
  }),
}));
mock.module('../../src/components/Sidebar/SidebarProvider', () => ({
  useSidebar: () => ({ serverAddress: '', startSummaryPolling: () => {}, stopSummaryPolling: () => {} }),
}));
mock.module('framer-motion', () => ({ motion: { div: ({ children }: { children?: ReactNode }) => <div>{children}</div> } }));
const notify = mock(() => {});
mock.module('sonner', () => ({ toast: { info: notify, error: notify, success: notify, warning: notify } }));
let params = new URLSearchParams('id=meeting-a');
mock.module('next/navigation', () => ({ ...originals.navigation, useSearchParams: () => params, useRouter: () => ({ push: () => {} }) }));
let recordingState = { isRecording: false, isStopping: false, isProcessing: false, isSaving: false, activeDuration: null };
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals.recordingState, useRecordingState: () => recordingState }));
mock.module('../../src/lib/summary-language-preferences', () => ({
  readCachedDetectedSummaryLanguage: async () => null,
  detectAndCacheSummaryLanguage: async () => ({ language: 'en' }),
  readMeetingSummaryLanguage: async () => ({ language: null, storage: 'metadata' }),
  saveMeetingSummaryLanguage: async (_id: string, language: string | null) => ({ language, storage: 'metadata' }),
}));

// Summary panel stand-ins: the panel's own children are not under test here
const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
// The stand-ins keep their props, so a test can press each summary start path the way the user would
mock.module('../../src/components/AISummary/BlockNoteSummaryView', () => ({
  BlockNoteSummaryView: forwardRef((_props, _ref) => { return null; }),
}));
mock.module('../../src/components/EmptyStateSummary', () => ({
  EmptyStateSummary: (props: { onGenerate: () => void }) => { return null; },
}));
mock.module('../../src/components/MeetingDetails/SummaryGeneratorButtonGroup', () => ({
  SummaryGeneratorButtonGroup: () => null,
}));
mock.module('../../src/components/MeetingDetails/SummaryUpdaterButtonGroup', () => ({ SummaryUpdaterButtonGroup: () => null }));
mock.module('../../src/components/ui/popover', () => ({ Popover: passThrough, PopoverTrigger: passThrough, PopoverContent: passThrough }));
mock.module('../../src/components/LanguagePickerPopover', () => ({ LanguagePickerPopover: () => null }));
mock.module('../../src/hooks/useRecentLanguages', () => ({ useRecentLanguages: () => ({ addRecent: () => {} }) }));

// The transcript panel is stubbed; the page's speaker state reaches the test through its props
let transcriptProps: { onResumeRecording?: () => void; speakers: { name: (idx: number, target: { personId: string }) => Promise<string | null>; namesChanged: boolean } } = {
  speakers: { name: async () => null, namesChanged: false },
};
mock.module('../../src/components/MeetingDetails/TranscriptPanel', () => ({
  TranscriptPanel: (props: typeof transcriptProps) => { transcriptProps = props; return null; },
}));
// The page shell (rail, back crumb, Ask toggle) is not under test here
mock.module('../../src/components/MeetingPage/MeetingRail', () => ({ MeetingRail: () => null }));
mock.module('../../src/components/BackCrumb', () => ({ BackCrumb: () => null }));
mock.module('../../src/components/AiSidebar/AiSidebarProvider', () => ({ ...originals.aiSidebar, useAiSidebar: () => ({ open: false, toggle: () => {} }) }));
mock.module('../../src/components/MeetingPage/FinishedTabs', () => ({
  FinishedTabs: (props: { transcript: ReactNode; summary: ReactNode }) => <>{props.transcript}{props.summary}</>,
}));
mock.module('../../src/components/MeetingDetails/AudioPlayerBar', () => ({ AudioPlayerBar: () => null }));
mock.module('../../src/components/MeetingNotes/NotesPane', () => ({ NotesPane: () => null }));

mock.module('../../src/hooks/meeting-details/useMeetingData', () => ({
  useMeetingData: () => ({
    transcripts: transcriptRows,
    aiSummary: null,
    setAiSummary: () => {},
    meetingTitle: 'Meeting A',
    blockNoteSummaryRef: { current: null },
    isSaving: false,
    isSummaryDirty: false,
    setIsSummaryDirty: () => {},
    saveAllChanges: async () => false,
    handleSaveSummary: async () => {},
    handleSummaryChange: () => {},
    updateMeetingTitle: () => {},
  }),
}));
mock.module('../../src/hooks/meeting-details/useTemplates', () => ({
  useTemplates: () => ({ availableTemplates: [], selectedTemplate: null, handleTemplateSelection: () => {} }),
}));
mock.module('../../src/hooks/meeting-details/useMeetingOperations', () => ({
  useMeetingOperations: () => ({ handleOpenMeetingFolder: async () => {} }),
}));
mock.module('../../src/hooks/useMeetingNotes', () => ({
  useMeetingNotes: () => ({ notes: [], loaded: true, addNote: () => {}, updateNote: () => {}, deleteNote: () => {} }),
}));
mock.module('../../src/hooks/useMeetingAudio', () => ({
  useMeetingAudio: () => ({ info: { resumable: true }, src: null, audioRef: { current: null }, audioEvents: {}, playFrom: () => {}, playhead: null }),
}));
mock.module('../../src/hooks/useIsDesktop', () => ({ useIsDesktop: () => true }));
mock.module('../../src/hooks/useRecordingStart', () => ({ useRecordingStart: () => ({ handleRecordingStart: async () => {}, isAutoStarting: false }) }));
mock.module('../../src/hooks/useModalState', () => ({ useModalState: () => ({ modals: {}, messages: {}, showModal() {}, hideModal() {} }) }));
mock.module('../../src/app/_components/SettingsModal', () => ({ SettingsModals: () => null }));
mock.module('../../src/components/MeetingPage/MeetingHeader', () => ({ MeetingHeader: () => null }));

const { default: MeetingDetails } = await import('../../src/app/meeting-details/page');

let renderer: ReactTestRenderer | undefined;
const sleep = (ms: number) => new Promise<void>((done) => realSetTimeout(done, ms));
const settle = () => act(async () => { await sleep(80); });
const processCalls = () => invoke.mock.calls.filter(([command]) => command === 'api_process_transcript');
const update = async () => { await act(async () => { const v = <MeetingDetails />; if (renderer) renderer.update(v); else renderer = create(v); }); await settle(); };

beforeEach(() => {
  params = new URLSearchParams('id=meeting-a');
  settings = { speakerIdActive: false, speakerSettingsLoaded: true };
  jobReply = { job: null, speakers: [] };
  transcriptRows = [{ id: 'line-a', text: 'Hello', timestamp: '10:00:05', audio_start_time: 5, sequence_id: 1 }];
  invoke.mockClear();
});
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  globalThis.setTimeout = realSetTimeout;
});

const arriveFromRecording = async () => {
  await update();
  expect(processCalls().length).toBe(0); // opened from the list: no auto-summary
  params = new URLSearchParams('id=meeting-a&source=recording'); // View Meeting after a stop, page already mounted
  await update();
};

test('arriving with source=recording on an open meeting page starts the summary once', async () => {
  await arriveFromRecording();
  expect(processCalls().length).toBe(1);
  await update(); // a re-render does not start it again
  expect(processCalls().length).toBe(1);
});

test('with speaker analysis pending it waits, then starts once the job finishes', async () => {
  settings = { speakerIdActive: true, speakerSettingsLoaded: true };
  jobReply = { job: { status: 'pending', reason: null, percent: null }, speakers: [] };
  await arriveFromRecording();
  expect(processCalls().length).toBe(0);
  jobReply = { job: { status: 'done', reason: null, percent: null }, speakers: [] };
  await act(async () => { handlers.get('speakers-updated')?.({ payload: { meeting_id: 'meeting-a' } }); });
  await settle();
  expect(processCalls().length).toBe(1);
});
