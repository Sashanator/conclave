import { afterAll, afterEach, beforeEach, expect, mock, test } from 'bun:test';
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
  audioBar: { ...await import('../../src/components/MeetingDetails/AudioPlayerBar') },
  notesPane: { ...await import('../../src/components/MeetingNotes/NotesPane') },
  meetingData: { ...await import('../../src/hooks/meeting-details/useMeetingData') },
  templates: { ...await import('../../src/hooks/meeting-details/useTemplates') },
  meetingOperations: { ...await import('../../src/hooks/meeting-details/useMeetingOperations') },
  notes: { ...await import('../../src/hooks/useMeetingNotes') },
  audio: { ...await import('../../src/hooks/useMeetingAudio') },
  desktop: { ...await import('../../src/hooks/useIsDesktop') },
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
  mock.module('../../src/components/MeetingDetails/AudioPlayerBar', () => originals.audioBar);
  mock.module('../../src/components/MeetingNotes/NotesPane', () => originals.notesPane);
  mock.module('../../src/hooks/meeting-details/useMeetingData', () => originals.meetingData);
  mock.module('../../src/hooks/meeting-details/useTemplates', () => originals.templates);
  mock.module('../../src/hooks/meeting-details/useMeetingOperations', () => originals.meetingOperations);
  mock.module('../../src/hooks/useMeetingNotes', () => originals.notes);
  mock.module('../../src/hooks/useMeetingAudio', () => originals.audio);
  mock.module('../../src/hooks/useIsDesktop', () => originals.desktop);
  mock.module('../../src/components/MeetingPage/MeetingHeader', () => originals.header);
});

const realSetTimeout = globalThis.setTimeout;
const sleep = (ms: number) => new Promise<void>((done) => realSetTimeout(done, ms));

// Timers of exactly 5 s are held until a test fires them; every other timer is real
const heldFiveSecondTimers: Array<() => void> = [];
function holdFiveSecondTimers() {
  globalThis.setTimeout = ((callback: () => void, ms?: number, ...rest: unknown[]) => {
    if (ms === 5000) { heldFiveSecondTimers.push(callback); return 0 as unknown as ReturnType<typeof setTimeout>; }
    return realSetTimeout(callback, ms, ...rest);
  }) as typeof setTimeout;
}

type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();
let jobReply: MeetingSpeakers = { job: null, speakers: [] };
let lookupDelayMs = 0;
let transcriptRows: Partial<Transcript>[] = [];
const invoke = mock(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'speakers_get_meeting') { await sleep(lookupDelayMs); return jobReply; }
  if (command === 'speakers_name') return 'p1';
  if (command === 'api_get_meeting_transcripts') return { transcripts: transcriptRows, total_count: transcriptRows.length, has_more: false };
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
    modelConfig: { provider: 'claude', model: 'claude-haiku-5-5', whisperModel: 'large-v3-turbo', ollamaEndpoint: null },
    setModelConfig: () => {},
    isModelConfigLoading: false,
    ...settings,
  }),
}));
mock.module('../../src/components/Sidebar/SidebarProvider', () => ({
  useSidebar: () => ({ serverAddress: '', refetchMeetings, startSummaryPolling: () => {}, stopSummaryPolling: () => {} }),
}));
mock.module('framer-motion', () => ({ motion: { div: ({ children }: { children?: ReactNode }) => <div>{children}</div> } }));
const notify = mock(() => {});
const push = mock((_u: string) => {});
const refetchMeetings = mock(async () => {});
const reload = mock(() => {});
mock.module('sonner', () => ({ toast: { info: notify, error: notify, success: notify, warning: notify } }));
mock.module('next/navigation', () => ({ ...originals.navigation, useRouter: () => ({ push }) }));
let recordingState: Record<string, unknown> = { isRecording: false, isStopping: false, isProcessing: false, isSaving: false, activeDuration: null, liveMeetingId: null };
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
let generatorProps: { onGenerateSummary: (prompt: string) => Promise<void>; onTemplateSelect: (id: string, name: string) => void } | undefined;
let blockNoteProps: { onRegenerateSummary?: () => void } | undefined;
let emptyStateProps: { onGenerate: () => void } | undefined;
mock.module('../../src/components/AISummary/BlockNoteSummaryView', () => ({
  BlockNoteSummaryView: forwardRef((props: { onRegenerateSummary?: () => void }, _ref) => { blockNoteProps = props; return null; }),
}));
mock.module('../../src/components/EmptyStateSummary', () => ({
  EmptyStateSummary: (props: { onGenerate: () => void }) => { emptyStateProps = props; return null; },
}));
mock.module('../../src/components/MeetingDetails/SummaryGeneratorButtonGroup', () => ({
  SummaryGeneratorButtonGroup: (props: typeof generatorProps & object) => { generatorProps = props as typeof generatorProps; return null; },
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
mock.module('../../src/components/MeetingPage/FinishedTabs', () => ({
  FinishedTabs: (props: { transcript: ReactNode; summary: ReactNode }) => <>{props.transcript}{props.summary}</>,
}));
mock.module('../../src/components/MeetingDetails/AudioPlayerBar', () => ({ AudioPlayerBar: () => null }));
mock.module('../../src/components/MeetingNotes/NotesPane', () => ({ NotesPane: () => null }));

const updateMeetingTitle = mock((_t: string) => {});
const handleTemplateSelection = mock((_id: string, _name: string) => {});
type HeaderProps = { live?: boolean; onChange: (r: unknown) => void; onMeetingDeleted: () => void; onRecordingDeleted: () => void; onRegenerateSummary: (id: string) => void };
let headerProps: HeaderProps | undefined;
mock.module('../../src/components/MeetingPage/MeetingHeader', () => ({ MeetingHeader: (p: HeaderProps) => { headerProps = p; return null; } }));
let dirty = false;
let aiSummary: { markdown: string } | null = null;
mock.module('../../src/hooks/meeting-details/useMeetingData', () => ({
  useMeetingData: () => ({
    transcripts: transcriptRows,
    aiSummary,
    setAiSummary: () => {},
    meetingTitle: 'Meeting A',
    blockNoteSummaryRef: { current: null },
    isSaving: false,
    isSummaryDirty: dirty,
    setIsSummaryDirty: () => {},
    saveAllChanges: async () => false,
    handleSaveSummary: async () => {},
    handleSummaryChange: () => {},
    updateMeetingTitle,
  }),
}));
mock.module('../../src/hooks/meeting-details/useTemplates', () => ({
  useTemplates: () => ({ availableTemplates: [{ id: 't1', name: 'Interview' }], selectedTemplate: null, handleTemplateSelection }),
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

const { default: PageContent } = await import('../../src/app/meeting-details/page-content');

const meeting = { id: 'meeting-a', title: 'Meeting A', created_at: '2026-10-14T00:00:00Z', folder_path: null };
const row = { id: 'meeting-a', title: 'Meeting A', startedAt: '2026-11-16 10:00:00+00:00', endedAt: '2026-11-16 11:00:00+00:00', hasTranscript: true };
const realLocation = window.location;
let renderer: ReactTestRenderer | undefined;
let onRowChange = mock((_r: unknown) => {});
let onMeetingUpdated = mock(async () => {});
beforeEach(() => {
  recordingState = { isRecording: false, isStopping: false, isProcessing: false, isSaving: false, activeDuration: null, liveMeetingId: null };
  settings = { speakerIdActive: false, speakerSettingsLoaded: true };
  transcriptRows = []; aiSummary = null; dirty = false; headerProps = undefined;
  push.mockClear(); refetchMeetings.mockClear(); reload.mockClear(); updateMeetingTitle.mockClear(); handleTemplateSelection.mockClear();
  onRowChange = mock((_r: unknown) => {}); onMeetingUpdated = mock(async () => {});
  Object.defineProperty(window, 'location', { configurable: true, value: { ...realLocation, reload } });
});
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  Object.defineProperty(window, 'location', { configurable: true, value: realLocation });
});
async function show() {
  await act(async () => {
    renderer = create(
      <PageContent meeting={meeting} summaryData={null} initialSummary={null} shouldAutoGenerate={false} row={row as never}
        onRowChange={onRowChange} onMeetingUpdated={onMeetingUpdated} />,
    );
  });
}

test('Delete meeting refetches the list then leaves for it', async () => {
  await show();
  await act(async () => { headerProps!.onMeetingDeleted(); await sleep(0); });
  expect(refetchMeetings).toHaveBeenCalled();
  expect(push).toHaveBeenCalledWith('/');
});

test('Delete recording reloads the page', async () => {
  await show();
  headerProps!.onRecordingDeleted();
  expect(reload).toHaveBeenCalled();
});

test('another meeting recording does not mark a finished meeting live', async () => {
  recordingState = { ...recordingState, isRecording: true, liveMeetingId: 'other' };
  await show();
  expect(headerProps!.live).toBe(false);
});

test('this meeting being the live target marks it live', async () => {
  recordingState = { ...recordingState, isRecording: true, liveMeetingId: 'meeting-a' };
  await show();
  expect(headerProps!.live).toBe(true);
});

test('a category regenerate from the toast also picks the template', async () => {
  await show();
  await act(async () => { headerProps!.onRegenerateSummary('t1'); });
  expect(handleTemplateSelection).toHaveBeenCalledWith('t1', 'Interview');
});

test('a header rename reaches the meeting data too', async () => {
  await show();
  await act(async () => { headerProps!.onChange({ ...row, title: 'Renamed' }); });
  expect(onRowChange).toHaveBeenCalled();
  expect(updateMeetingTitle).toHaveBeenCalledWith('Renamed');
});

test('an unchanged title does not touch the meeting data', async () => {
  await show();
  await act(async () => { headerProps!.onChange({ ...row, templateId: 'x' }); });
  expect(updateMeetingTitle).not.toHaveBeenCalled();
});
