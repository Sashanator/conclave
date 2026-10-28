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
  preferences: { ...await import('../../src/lib/summary-language-preferences') },
  blockNote: { ...await import('../../src/components/AISummary/BlockNoteSummaryView') },
  empty: { ...await import('../../src/components/EmptyStateSummary') },
  generator: { ...await import('../../src/components/MeetingDetails/SummaryGeneratorButtonGroup') },
  updater: { ...await import('../../src/components/MeetingDetails/SummaryUpdaterButtonGroup') },
  popover: { ...await import('../../src/components/ui/popover') },
  picker: { ...await import('../../src/components/LanguagePickerPopover') },
  recents: { ...await import('../../src/hooks/useRecentLanguages') },
  transcriptPanel: { ...await import('../../src/components/MeetingDetails/TranscriptPanel') },
  splitView: { ...await import('../../src/components/MeetingDetails/MeetingDetailsSplitView') },
  audioBar: { ...await import('../../src/components/MeetingDetails/AudioPlayerBar') },
  notesPane: { ...await import('../../src/components/MeetingNotes/NotesPane') },
  meetingData: { ...await import('../../src/hooks/meeting-details/useMeetingData') },
  templates: { ...await import('../../src/hooks/meeting-details/useTemplates') },
  meetingOperations: { ...await import('../../src/hooks/meeting-details/useMeetingOperations') },
  notes: { ...await import('../../src/hooks/useMeetingNotes') },
  audio: { ...await import('../../src/hooks/useMeetingAudio') },
  desktop: { ...await import('../../src/hooks/useIsDesktop') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('../../src/contexts/ConfigContext', () => originals.config);
  mock.module('../../src/components/Sidebar/SidebarProvider', () => originals.sidebar);
  mock.module('framer-motion', () => originals.motion);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/lib/summary-language-preferences', () => originals.preferences);
  mock.module('../../src/components/AISummary/BlockNoteSummaryView', () => originals.blockNote);
  mock.module('../../src/components/EmptyStateSummary', () => originals.empty);
  mock.module('../../src/components/MeetingDetails/SummaryGeneratorButtonGroup', () => originals.generator);
  mock.module('../../src/components/MeetingDetails/SummaryUpdaterButtonGroup', () => originals.updater);
  mock.module('../../src/components/ui/popover', () => originals.popover);
  mock.module('../../src/components/LanguagePickerPopover', () => originals.picker);
  mock.module('../../src/hooks/useRecentLanguages', () => originals.recents);
  mock.module('../../src/components/MeetingDetails/TranscriptPanel', () => originals.transcriptPanel);
  mock.module('../../src/components/MeetingDetails/MeetingDetailsSplitView', () => originals.splitView);
  mock.module('../../src/components/MeetingDetails/AudioPlayerBar', () => originals.audioBar);
  mock.module('../../src/components/MeetingNotes/NotesPane', () => originals.notesPane);
  mock.module('../../src/hooks/meeting-details/useMeetingData', () => originals.meetingData);
  mock.module('../../src/hooks/meeting-details/useTemplates', () => originals.templates);
  mock.module('../../src/hooks/meeting-details/useMeetingOperations', () => originals.meetingOperations);
  mock.module('../../src/hooks/useMeetingNotes', () => originals.notes);
  mock.module('../../src/hooks/useMeetingAudio', () => originals.audio);
  mock.module('../../src/hooks/useIsDesktop', () => originals.desktop);
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
  useSidebar: () => ({ serverAddress: '', startSummaryPolling: () => {}, stopSummaryPolling: () => {} }),
}));
mock.module('framer-motion', () => ({ motion: { div: ({ children }: { children?: ReactNode }) => <div>{children}</div> } }));
const notify = mock(() => {});
mock.module('sonner', () => ({ toast: { info: notify, error: notify, success: notify, warning: notify } }));
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
let transcriptProps: { speakers: { name: (idx: number, target: { personId: string }) => Promise<string | null>; namesChanged: boolean } } = {
  speakers: { name: async () => null, namesChanged: false },
};
mock.module('../../src/components/MeetingDetails/TranscriptPanel', () => ({
  TranscriptPanel: (props: typeof transcriptProps) => { transcriptProps = props; return null; },
}));
mock.module('../../src/components/MeetingDetails/MeetingDetailsSplitView', () => ({
  MeetingDetailsSplitView: (props: { transcript: ReactNode; summary: ReactNode }) => <>{props.transcript}{props.summary}</>,
}));
mock.module('../../src/components/MeetingDetails/AudioPlayerBar', () => ({ AudioPlayerBar: () => null }));
mock.module('../../src/components/MeetingNotes/NotesPane', () => ({ NotesPane: () => null }));

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
    saveAllChanges: async () => {},
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
  useMeetingAudio: () => ({ info: null, src: null, audioRef: { current: null }, audioEvents: {}, playFrom: () => {}, playhead: null }),
}));
mock.module('../../src/hooks/useIsDesktop', () => ({ useIsDesktop: () => true }));

const { default: PageContent } = await import('../../src/app/meeting-details/page-content');
const { useCopyOperations } = await import('../../src/hooks/meeting-details/useCopyOperations');

const SERGEY = { idx: 1, name: 'Сергей', person_id: 'p1', is_self: false, seconds: 1080 };
const line = (extra: Partial<Transcript> = {}): Partial<Transcript> => ({
  id: 'line-a', text: 'Hello', timestamp: '10:00:05', audio_start_time: 5, sequence_id: 1, ...extra,
});
const meeting = { id: 'meeting-a', title: 'Meeting A', created_at: '2026-10-14T00:00:00Z', folder_path: null };
const job = (status: 'pending' | 'running' | 'done' | 'failed' | 'skipped'): MeetingSpeakers['job'] => ({
  status, reason: status === 'failed' ? 'boom' : null, percent: null,
});

let renderer: ReactTestRenderer | undefined;
let autoGenerate = true;
let regenerateOnAuto = false;
let initialSummary: SummaryProcessResponse | null = null;
let onAutoGenerateComplete = mock(() => {});
beforeEach(() => {
  settings = { speakerIdActive: false, speakerSettingsLoaded: true };
  jobReply = { job: null, speakers: [] };
  lookupDelayMs = 0;
  transcriptRows = [line()];
  aiSummary = null;
  dirty = false;
  autoGenerate = true;
  regenerateOnAuto = false;
  initialSummary = null;
  onAutoGenerateComplete = mock(() => {});
  handlers.clear();
  heldFiveSecondTimers.length = 0;
  invoke.mockClear();
  notify.mockClear();
  holdFiveSecondTimers();
});
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  globalThis.setTimeout = realSetTimeout;
});

async function show() {
  await act(async () => {
    const view = (
      <PageContent
        meeting={meeting}
        summaryData={null}
        initialSummary={initialSummary}
        shouldAutoGenerate={autoGenerate}
        regenerateOnAuto={regenerateOnAuto}
        onAutoGenerateComplete={onAutoGenerateComplete}
      />
    );
    if (renderer) renderer.update(view); else renderer = create(view);
  });
}
async function flush() {
  await act(async () => { await sleep(0); });
}
// Lets a lookup of up to 50 ms answer, then the React updates it caused
async function settle() {
  await act(async () => { await sleep(70); });
}
async function emit(event: string, payload: unknown) {
  await act(async () => { handlers.get(event)?.({ payload }); });
}
function processCalls(): Array<Record<string, unknown>> {
  return invoke.mock.calls.filter(([command]) => command === 'api_process_transcript').map(([, args]) => args as Record<string, unknown>);
}
const text = () => JSON.stringify(renderer!.toJSON());
function textOf(node: ReactTestInstance | string): string {
  return typeof node === 'string' ? node : node.children.map(textOf).join('');
}
function button(label: string): ReactTestInstance {
  const found = renderer!.root.findAll((n) => n.type === 'button' && textOf(n) === label);
  if (found.length === 0) throw new Error(`no button "${label}"`);
  return found[0];
}
async function click(label: string) {
  await act(async () => { button(label).props.onClick(); });
}

describe('continued meeting summary', () => {
  const completed: SummaryProcessResponse = {
    status: 'completed', meeting_id: 'meeting-a', meetingName: null, start: null, end: null, data: { markdown: 'Old summary' }, error: null,
  };

  test('a continued meeting with a completed summary regenerates it', async () => {
    initialSummary = completed;
    regenerateOnAuto = true;
    await show();
    await flush();
    expect(processCalls().length).toBe(1);
  });

  test('without regenerateOnAuto a completed summary is not regenerated', async () => {
    initialSummary = completed;
    await show();
    await flush();
    expect(processCalls().length).toBe(0);
  });
});

describe('summary waits for speaker analysis', () => {
  test('feature off starts the summary at once and shows no waiting text', async () => {
    await show();
    await flush();
    expect(processCalls().length).toBe(1);
    expect(text()).not.toContain('Waiting for speaker analysis');
  });

  test('feature off keeps today’s transcript text, names never appear', async () => {
    transcriptRows = [line({ speaker_idx: 1 })];
    await show();
    await flush();
    expect(processCalls()[0].text).toBe('[00:05] Hello');
  });

  test('feature on with no analysis yet: an unnamed line keeps today’s text', async () => {
    settings = { speakerIdActive: true, speakerSettingsLoaded: true };
    transcriptRows = [line()];
    await show();
    await flush();
    expect(processCalls()[0].text).toBe('[00:05] Hello');
  });

  test('settings still loading: nothing starts; the settings answer after 50 ms, a pending job keeps it closed', async () => {
    settings = { speakerIdActive: false, speakerSettingsLoaded: false };
    jobReply = { job: job('pending'), speakers: [] };
    await show();
    await flush();
    expect(processCalls().length).toBe(0);
    await sleep(50);
    expect(processCalls().length).toBe(0);
    settings = { speakerIdActive: true, speakerSettingsLoaded: true };
    await show();
    await settle();
    await flush();
    expect(processCalls().length).toBe(0);
    expect(text()).toContain('Waiting for speaker analysis…');
  });

  test('first lookup answers after 50 ms with a pending job: nothing starts before or after it answers', async () => {
    settings = { speakerIdActive: true, speakerSettingsLoaded: true };
    lookupDelayMs = 50;
    jobReply = { job: job('pending'), speakers: [] };
    await show();
    await flush();
    expect(processCalls().length).toBe(0);
    await settle();
    expect(processCalls().length).toBe(0);
    expect(text()).toContain('Waiting for speaker analysis…');
  });

  test.each(['pending', 'running'] as const)('a %s job holds the summary', async (status) => {
    settings = { speakerIdActive: true, speakerSettingsLoaded: true };
    jobReply = { job: job(status), speakers: [] };
    await show();
    await settle();
    await emit('speakers-updated', { meeting_id: 'meeting-a' });
    await settle();
    expect(processCalls().length).toBe(0);
  });

  test.each(['done', 'failed', 'skipped'] as const)('a job that ends %s opens the gate after speakers-updated', async (status) => {
    settings = { speakerIdActive: true, speakerSettingsLoaded: true };
    jobReply = { job: job('pending'), speakers: [] };
    await show();
    await settle();
    expect(processCalls().length).toBe(0);
    jobReply = { job: job(status), speakers: [] };
    await emit('speakers-updated', { meeting_id: 'meeting-a' });
    await settle();
    await flush();
    expect(processCalls().length).toBe(1);
    expect(text()).not.toContain('Waiting for speaker analysis');
  });

  test('a done job names the lines in the summary text', async () => {
    settings = { speakerIdActive: true, speakerSettingsLoaded: true };
    transcriptRows = [line({ speaker_idx: 1, mark: 'action' })];
    jobReply = { job: job('done'), speakers: [SERGEY] };
    await show();
    await settle();
    await flush();
    expect(processCalls()[0].text).toBe('[00:05] [MARKED: action] Сергей: Hello');
  });

  test('Summarize now starts the summary while the job is still pending', async () => {
    settings = { speakerIdActive: true, speakerSettingsLoaded: true };
    jobReply = { job: job('pending'), speakers: [] };
    await show();
    await settle();
    expect(processCalls().length).toBe(0);
    await click('Summarize now');
    await flush();
    expect(processCalls().length).toBe(1);
    expect(text()).not.toContain('Waiting for speaker analysis');
  });

  test('settings that never answer open the gate as inactive after 5 s', async () => {
    settings = { speakerIdActive: false, speakerSettingsLoaded: false };
    await show();
    await flush();
    expect(processCalls().length).toBe(0);
    expect(heldFiveSecondTimers.length).toBe(1);
    await act(async () => { heldFiveSecondTimers[0](); });
    await flush();
    expect(processCalls().length).toBe(1);
  });
});

describe('gate fixes', () => {
  test('a settings answer that arrives late as active still names the lines', async () => {
    autoGenerate = false;
    aiSummary = { markdown: 'Existing summary' };
    transcriptRows = [line({ speaker_idx: 1 })];
    settings = { speakerIdActive: false, speakerSettingsLoaded: false };
    await show();
    await act(async () => { heldFiveSecondTimers[0](); });
    settings = { speakerIdActive: true, speakerSettingsLoaded: true };
    jobReply = { job: job('done'), speakers: [SERGEY] };
    await show();
    await settle();
    await act(async () => { await transcriptProps.speakers.name(1, { personId: 'p1' }); });
    await click('Regenerate');
    await flush();
    expect(processCalls().at(-1)!.text).toBe('[00:05] Сергей: Hello');
  });

  test('Summarize now starts the summary while the first lookup is still in flight', async () => {
    settings = { speakerIdActive: true, speakerSettingsLoaded: true };
    lookupDelayMs = 200;
    jobReply = { job: job('pending'), speakers: [] };
    await show();
    await flush();
    expect(text()).toContain('Waiting for speaker analysis…');
    await click('Summarize now');
    await flush();
    expect(processCalls().length).toBe(1);
  });

  test('Summarize now starts the summary while the settings lookup is still pending', async () => {
    settings = { speakerIdActive: false, speakerSettingsLoaded: false };
    await show();
    await flush();
    expect(text()).toContain('Waiting for speaker analysis…');
    await click('Summarize now');
    await flush();
    expect(processCalls().length).toBe(1);
  });

  test('a failed re-identify keeps the published names in the summary', async () => {
    settings = { speakerIdActive: true, speakerSettingsLoaded: true };
    transcriptRows = [line({ speaker_idx: 1 })];
    jobReply = { job: job('failed'), speakers: [SERGEY] };
    await show();
    await settle();
    await flush();
    expect(processCalls()[0].text).toBe('[00:05] Сергей: Hello');
  });

  test('an auto-summary clears the names-changed state', async () => {
    settings = { speakerIdActive: true, speakerSettingsLoaded: true };
    jobReply = { job: job('pending'), speakers: [] };
    await show();
    await settle();
    await act(async () => { await transcriptProps.speakers.name(1, { personId: 'p1' }); });
    expect(transcriptProps.speakers.namesChanged).toBe(true);
    jobReply = { job: job('done'), speakers: [SERGEY] };
    await emit('speakers-updated', { meeting_id: 'meeting-a' });
    await settle();
    await flush();
    expect(processCalls().length).toBe(1);
    expect(transcriptProps.speakers.namesChanged).toBe(false);
  });

  test.each([
    ['the header Generate button', true, () => generatorProps!.onGenerateSummary('')],
    ['a template pick that regenerates', true, () => generatorProps!.onTemplateSelect('daily_standup', 'Daily standup')],
    ['Regenerate in the summary view', true, () => blockNoteProps!.onRegenerateSummary!()],
    ['the empty-state Generate button', false, () => emptyStateProps!.onGenerate()],
  ])('%s clears the names-changed state', async (_label, hasSummary, start) => {
    aiSummary = hasSummary ? { markdown: 'Existing summary' } : null;
    settings = { speakerIdActive: true, speakerSettingsLoaded: true };
    jobReply = { job: job('done'), speakers: [SERGEY] };
    autoGenerate = false;
    await show();
    await settle();
    await act(async () => { await transcriptProps.speakers.name(1, { personId: 'p1' }); });
    expect(transcriptProps.speakers.namesChanged).toBe(true);
    await act(async () => { await start(); });
    await flush();
    expect(processCalls().length).toBe(1);
    expect(transcriptProps.speakers.namesChanged).toBe(false);
  });
});

describe('speaker names changed hint', () => {
  test('the hint shows after a rename and Regenerate runs through the guard', async () => {
    aiSummary = { markdown: 'Existing summary' };
    settings = { speakerIdActive: true, speakerSettingsLoaded: true };
    jobReply = { job: job('done'), speakers: [SERGEY] };
    autoGenerate = false;
    await show();
    await settle();
    expect(text()).not.toContain('Speaker names changed');
    await act(async () => { await transcriptProps.speakers.name(1, { personId: 'p1' }); });
    expect(text()).toContain('Speaker names changed. Regenerate the summary to use them');
    const before = processCalls().length;
    await click('Regenerate');
    await flush();
    expect(processCalls().length).toBe(before + 1);
    expect(text()).not.toContain('Speaker names changed');
  });

  test('a declined discard keeps the summary and the hint', async () => {
    aiSummary = { markdown: 'Existing summary' };
    dirty = true;
    settings = { speakerIdActive: true, speakerSettingsLoaded: true };
    jobReply = { job: job('done'), speakers: [SERGEY] };
    autoGenerate = false;
    const confirm = spyOn(window, 'confirm').mockImplementation(() => false);
    try {
      await show();
      await settle();
      await act(async () => { await transcriptProps.speakers.name(1, { personId: 'p1' }); });
      const before = processCalls().length;
      await click('Regenerate');
      await flush();
      expect(confirm).toHaveBeenCalledTimes(1);
      expect(processCalls().length).toBe(before);
      expect(text()).toContain('Speaker names changed');
    } finally {
      confirm.mockRestore();
    }
  });
});

describe('copy transcript speaker names', () => {
  let copy: () => Promise<void>;
  function CopyView({ speakerName }: { speakerName?: (t: Transcript) => string | undefined }) {
    copy = useCopyOperations({
      meeting, transcripts: [], meetingTitle: 'Meeting A', aiSummary: null,
      blockNoteSummaryRef: { current: null }, speakerName,
    }).handleCopyTranscript;
    return null;
  }
  async function copied(speakerName?: (t: Transcript) => string | undefined): Promise<string> {
    const writeText = mock(async (_text: string) => {});
    const clipboard = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    try {
      transcriptRows = [line({ audio_start_time: 724, text: 'Здравствуйте', speaker_idx: 1 })];
      await act(async () => { renderer = create(<CopyView speakerName={speakerName} />); });
      await act(async () => { await copy(); });
      return String(writeText.mock.calls[0][0]);
    } finally {
      if (clipboard) Object.defineProperty(navigator, 'clipboard', clipboard);
      else delete (navigator as { clipboard?: unknown }).clipboard;
    }
  }

  test('feature on: the line shows the name after the time', async () => {
    const names = (t: Transcript) => (t.speaker_idx === 1 ? 'Сергей' : undefined);
    expect(await copied(names)).toContain('[12:04] Сергей: Здравствуйте  ');
  });

  test('feature off: copy keeps today’s line', async () => {
    expect(await copied()).toContain('[12:04] Здравствуйте  ');
  });
});
