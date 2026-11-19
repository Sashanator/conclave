import { afterAll, beforeEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  sonner: { ...await import('sonner') },
  nav: { ...await import('next/navigation') },
  sidebar: { ...await import('../../src/components/Sidebar/SidebarProvider') },
  config: { ...await import('../../src/contexts/ConfigContext') },
  recording: { ...await import('../../src/contexts/RecordingStateContext') },
  paginated: { ...await import('../../src/hooks/usePaginatedTranscripts') },
  start: { ...await import('../../src/hooks/useRecordingStart') },
  modal: { ...await import('../../src/hooks/useModalState') },
  settings: { ...await import('../../src/app/_components/SettingsModal') },
  aiSidebar: { ...await import('../../src/components/AiSidebar/AiSidebarProvider') },
  content: { ...await import('../../src/app/meeting-details/page-content') },
  newView: { ...await import('../../src/components/MeetingPage/NewMeetingView') },
  recView: { ...await import('../../src/components/MeetingPage/RecordingView') },
  header: { ...await import('../../src/components/MeetingPage/MeetingHeader') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
  mock.module('next/navigation', () => originals.nav);
  mock.module('../../src/components/Sidebar/SidebarProvider', () => originals.sidebar);
  mock.module('../../src/contexts/ConfigContext', () => originals.config);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.recording);
  mock.module('../../src/hooks/usePaginatedTranscripts', () => originals.paginated);
  mock.module('../../src/hooks/useRecordingStart', () => originals.start);
  mock.module('../../src/hooks/useModalState', () => originals.modal);
  mock.module('../../src/app/_components/SettingsModal', () => originals.settings);
  mock.module('../../src/components/AiSidebar/AiSidebarProvider', () => originals.aiSidebar);
  mock.module('../../src/app/meeting-details/page-content', () => originals.content);
  mock.module('../../src/components/MeetingPage/NewMeetingView', () => originals.newView);
  mock.module('../../src/components/MeetingPage/RecordingView', () => originals.recView);
  mock.module('../../src/components/MeetingPage/MeetingHeader', () => originals.header);
});

const newRow = { id: 'm1', title: 'Weekly', startedAt: null, hasTranscript: false };
let rows: () => Promise<unknown[]> = async () => [newRow];
const invoke = mock(async (command: string) => (command === 'api_list_meeting_rows' ? rows() : command === 'api_get_summary' ? { status: 'idle', data: null } : null));
const toast = { error: mock((..._a: unknown[]) => {}), warning() {}, success() {}, info() {} };
const push = mock((_u: string) => {});
const recording: Record<string, unknown> = {};
let startArgs: unknown[] = [];
const refetchTranscripts = mock(() => {});
let screenKind: unknown;
const noRows: never[] = []; // stable: the page's metadata effect depends on it
let pageMetadata: unknown = null;
let autoStarting = false;
let startResult: () => Promise<void> = async () => {};
const handleRecordingStart = mock(async (_id?: unknown) => startResult());
let newProps: { disabled: boolean; onStart: () => void; header: React.ReactNode } | undefined;
let params = new URLSearchParams('id=m1');

mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('sonner', () => ({ toast }));
mock.module('next/navigation', () => ({ useSearchParams: () => params, useRouter: () => ({ push }) }));
mock.module('../../src/components/Sidebar/SidebarProvider', () => ({ useSidebar: () => ({ serverAddress: '' }) }));
mock.module('../../src/contexts/ConfigContext', () => ({ useConfig: () => ({ isAutoSummary: false, transcriptModelConfig: { provider: 'parakeet' } }) }));
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals.recording, useRecordingState: () => recording }));
mock.module('../../src/hooks/usePaginatedTranscripts', () => ({
  usePaginatedTranscripts: () => ({ metadata: pageMetadata, segments: noRows, transcripts: noRows, isLoading: false, isLoadingMore: false, hasMore: false, totalCount: 0, loadedCount: 0, loadMore() {}, refetch: refetchTranscripts, error: null, setTranscriptMark() {}, setTranscriptSpeaker() {} }),
}));
mock.module('../../src/hooks/useRecordingStart', () => ({
  useRecordingStart: (...args: unknown[]) => { startArgs = args; return { handleRecordingStart, isAutoStarting: autoStarting }; },
}));
mock.module('../../src/hooks/useModalState', () => ({ useModalState: () => ({ modals: {}, messages: {}, showModal() {}, hideModal() {} }) }));
mock.module('../../src/app/_components/SettingsModal', () => ({ SettingsModals: () => null }));
mock.module('../../src/components/AiSidebar/AiSidebarProvider', () => ({ ...originals.aiSidebar, useAiSidebarScreen: (kind: unknown) => { screenKind = kind; } }));
let contentProps: { onMeetingUpdated: () => Promise<void> } | undefined;
mock.module('../../src/app/meeting-details/page-content', () => ({ default: (p: NonNullable<typeof contentProps>) => { contentProps = p; return null; } }));
mock.module('../../src/components/MeetingPage/NewMeetingView', () => ({ NewMeetingView: (p: typeof newProps) => { newProps = p; return <div>New view{p!.header}</div>; } }));
mock.module('../../src/components/MeetingPage/RecordingView', () => ({ RecordingView: (p: { row: unknown }) => <div>{p.row ? 'Recording view' : 'Recording view unrouted'}</div> }));
let headerProps: { live?: boolean; onMeetingDeleted?: () => void } | undefined;
mock.module('../../src/components/MeetingPage/MeetingHeader', () => ({ MeetingHeader: (p: NonNullable<typeof headerProps>) => { headerProps = p; return null; } }));

const { default: MeetingDetails } = await import('../../src/app/meeting-details/page');

let renderer: ReactTestRenderer | undefined;
async function mount() {
  await act(async () => { renderer = create(<MeetingDetails />); });
  await act(async () => { await new Promise((r) => setTimeout(r, 10)); });
}
const rerender = async () => { await act(async () => { renderer!.update(<MeetingDetails />); }); };

beforeEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  rows = async () => [newRow];
  autoStarting = false; startResult = async () => {}; newProps = undefined; headerProps = undefined; contentProps = undefined; pageMetadata = null;
  params = new URLSearchParams('id=m1');
  Object.assign(recording, { liveMeetingId: null, isStopping: false, isProcessing: false, isSaving: false, isRecording: false, status: 'idle' });
  push.mockClear(); toast.error.mockClear(); handleRecordingStart.mockClear(); refetchTranscripts.mockClear();
  sessionStorage.clear(); screenKind = undefined;
});

test('a start that reached the backend does not leave the page busy once the recording ends', async () => {
  await mount();
  expect(newProps!.disabled).toBe(false);
  await act(async () => { (startArgs[1] as (v: boolean) => void)(true); }); // the hook marks a started recording
  expect(newProps!.disabled).toBe(true);
  recording.isRecording = true; await rerender();
  recording.isRecording = false; await rerender();
  expect(newProps!.disabled).toBe(false);
});

test('an auto-start in progress keeps the page busy', async () => {
  autoStarting = true;
  await mount();
  expect(newProps!.disabled).toBe(true);
});

test('a start error is shown, not swallowed', async () => {
  startResult = async () => { throw new Error('Microphone is in use'); };
  await mount();
  await act(async () => { newProps!.onStart(); });
  expect(toast.error).toHaveBeenCalledWith('Could not start recording', { description: 'Microphone is in use' });
});

test('the page key carries the start marker, so Resume on the same meeting is a new visit', async () => {
  params = new URLSearchParams('id=m1&start=123');
  await mount();
  expect(startArgs[3]).toBe('m1:123');
});

test('a released row that is gone sends the user home with a toast; one that remains is reloaded', async () => {
  await mount();
  rows = async () => [];
  await act(async () => { (startArgs[4] as () => void)(); });
  await act(async () => { await new Promise((r) => setTimeout(r, 10)); });
  expect(toast.error).toHaveBeenCalledWith('This meeting was removed because the recording could not start');
  expect(push).toHaveBeenCalledWith('/');
  push.mockClear();
  rows = async () => [newRow];
  await act(async () => { (startArgs[4] as () => void)(); });
  await act(async () => { await new Promise((r) => setTimeout(r, 10)); });
  expect(push).not.toHaveBeenCalled();
});

test('a row load failure is shown', async () => {
  rows = async () => { throw new Error('db locked'); };
  const logged = console.error; console.error = () => {};
  await mount();
  console.error = logged;
  expect(toast.error).toHaveBeenCalledWith('Could not load the meeting: Error: db locked');
});

const shows = () => JSON.stringify(renderer!.toJSON()).includes('Recording view');
const setState = (patch: Record<string, unknown>) => Object.assign(recording, patch);
const live = { liveMeetingId: 'm1', isRecording: true, status: 'recording' };
const stopping = { liveMeetingId: null, isRecording: false, isStopping: true, status: 'stopping' };
const saving = { isStopping: false, isSaving: true, status: 'saving' };
const done = { isSaving: false, isStopping: false, isProcessing: false, status: 'completed' };

test('live, stopping, saving, completed: the view holds until the save is done, then the page refetches', async () => {
  setState(live);
  await mount();
  expect(shows()).toBe(true);
  expect(screenKind).toBe('live');
  sessionStorage.setItem('last_recording_held_id', 'm1'); // the stop listener writes it
  setState(stopping); await rerender();
  expect(shows()).toBe(true);
  expect(screenKind).toBe('meeting'); // Copilot is live-only
  setState(saving); await rerender();
  expect(shows()).toBe(true);
  expect(refetchTranscripts).not.toHaveBeenCalled();
  sessionStorage.removeItem('last_recording_held_id'); // a successful save removes it
  setState(done); await rerender();
  await act(async () => { await new Promise((r) => setTimeout(r, 10)); });
  expect(shows()).toBe(false);
  expect(refetchTranscripts).toHaveBeenCalled();
});

test('a failed save keeps the view, also after a remount', async () => {
  setState(live);
  await mount();
  sessionStorage.setItem('last_recording_held_id', 'm1');
  setState(stopping); await rerender();
  setState({ isStopping: false, status: 'error' }); await rerender();
  expect(shows()).toBe(true);
  await act(async () => renderer!.unmount());
  renderer = undefined;
  await mount(); // remount with nothing but the shared state
  expect(shows()).toBe(true);
});

test('a remount during the save still shows the view', async () => {
  setState({ ...stopping, ...saving, isStopping: false });
  sessionStorage.setItem('last_recording_held_id', 'm1');
  await mount();
  expect(shows()).toBe(true);
});

test('a stop flow of another meeting does not take this page', async () => {
  setState({ ...stopping });
  sessionStorage.setItem('last_recording_held_id', 'm2');
  await mount();
  expect(shows()).toBe(false);
});

test('Delete meeting on a new meeting leaves for the list', async () => {
  await mount();
  await act(async () => { headerProps!.onMeetingDeleted!(); await new Promise((r) => setTimeout(r, 0)); });
  expect(push).toHaveBeenCalledWith('/');
});

test('the new meeting menu is disabled while a start is in flight', async () => {
  await mount();
  expect(headerProps!.live).toBe(false);
  await act(async () => { (startArgs[1] as (v: boolean) => void)(true); });
  expect(headerProps!.live).toBe(true);
});

test('a summary rename reloads the header row too', async () => {
  const finished = { id: 'm1', title: 'Weekly', startedAt: '2026-11-16 10:00:00+00:00', hasTranscript: true };
  rows = async () => [finished];
  await mount();
  pageMetadata = { id: 'm1', title: 'Weekly', created_at: '2026-11-16', updated_at: '2026-11-16', folder_path: null }; // arrives after the mount resets, as in the app
  await rerender();
  await act(async () => { await new Promise((r) => setTimeout(r, 20)); });
  const loads = () => invoke.mock.calls.filter(([c]) => c === 'api_list_meeting_rows').length;
  expect(contentProps).toBeDefined();
  const before = loads();
  await act(async () => { await contentProps?.onMeetingUpdated(); });
  expect(loads()).toBeGreaterThan(before);
});

const unroutedShows = () => JSON.stringify(renderer!.toJSON()).includes('Recording view unrouted');

test('a recording that has no meeting row shows the row-less view on the id-less page and Copilot', async () => {
  params = new URLSearchParams('');
  setState({ isRecording: true, status: 'recording' });
  await mount();
  expect(unroutedShows()).toBe(true);
  expect(screenKind).toBe('live');
});

test('the page that started an unrouted recording shows it, through the stop and save', async () => {
  await mount();
  await act(async () => { (startArgs[1] as (v: boolean) => void)(true); });
  setState({ isRecording: true, status: 'recording' }); await rerender();
  expect(unroutedShows()).toBe(true);
  setState({ isRecording: false, isSaving: true, status: 'saving' }); await rerender();
  expect(unroutedShows()).toBe(true);
  setState(done); await rerender();
  expect(unroutedShows()).toBe(false);
});

test('another meeting page does not show an unrouted recording', async () => {
  setState({ isRecording: true, status: 'recording' });
  await mount();
  expect(unroutedShows()).toBe(false);
});

test('a failed unrouted save does not follow the user to another meeting', async () => {
  await mount();
  await act(async () => { (startArgs[1] as (v: boolean) => void)(true); });
  setState({ isRecording: true, status: 'recording' }); await rerender();
  setState({ isRecording: false, isSaving: false, status: 'error' }); await rerender();
  expect(unroutedShows()).toBe(true);
  params = new URLSearchParams('id=m2'); await rerender();
  expect(unroutedShows()).toBe(false);
  expect(screenKind).not.toBe('live');
});
