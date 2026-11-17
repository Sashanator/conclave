import { afterAll, beforeEach, expect, mock, test } from 'bun:test';
import { useState } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  sonner: { ...await import('sonner') },
  nav: { ...await import('next/navigation') },
  sidebar: { ...await import('../../src/components/Sidebar/SidebarProvider') },
  recording: { ...await import('../../src/contexts/RecordingStateContext') },
  recovery: { ...await import('../../src/hooks/useTranscriptRecovery') },
  dialog: { ...await import('../../src/components/TranscriptRecovery') },
  idb: { ...await import('../../src/services/indexedDBService') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
  mock.module('next/navigation', () => originals.nav);
  mock.module('../../src/components/Sidebar/SidebarProvider', () => originals.sidebar);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.recording);
  mock.module('../../src/hooks/useTranscriptRecovery', () => originals.recovery);
  mock.module('../../src/components/TranscriptRecovery', () => originals.dialog);
  mock.module('../../src/services/indexedDBService', () => originals.idb);
});

const toast = { success: mock(() => {}), warning: mock(() => {}), error: mock(() => {}) };
const invoke = mock(async () => ({ cuda_runtime_missing: false }));
const meeting = { meetingId: 'm1', title: 'Standup' };
// The hook's list fills only when the check runs, as the real hook does.
let setRecoverable: ((list: unknown[]) => void) | null = null;
const check = mock(async () => { setRecoverable?.([meeting]); });
const recoverMeeting = mock(async (_id: string): Promise<unknown> => { throw new Error('disk full'); });
const deleteOld = mock(async () => {});

let recording = { isRecording: false, status: originals.recording.RecordingStatus.IDLE };
const loadMeetingTranscripts = mock(async () => []);
const deleteRecoverableMeeting = mock(async () => {});
let dialogProps: { isOpen: boolean; onRecover: (id: string) => Promise<void> } = { isOpen: false, onRecover: async () => {} };

mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));
mock.module('sonner', () => ({ toast }));
mock.module('next/navigation', () => ({ useRouter: () => ({ push: mock(() => {}) }) }));
mock.module('../../src/components/Sidebar/SidebarProvider', () => ({ useSidebar: () => ({ refetchMeetings: async () => {} }) }));
mock.module('../../src/contexts/RecordingStateContext', () => ({
  ...originals.recording,
  useRecordingState: () => recording,
}));
mock.module('../../src/hooks/useTranscriptRecovery', () => ({
  useTranscriptRecovery: () => {
    const [recoverableMeetings, set] = useState<unknown[]>([]);
    setRecoverable = set;
    return {
      recoverableMeetings, isLoading: false, isRecovering: false,
      checkForRecoverableTranscripts: check, recoverMeeting, loadMeetingTranscripts, deleteRecoverableMeeting,
    };
  },
}));
mock.module('../../src/components/TranscriptRecovery', () => ({
  TranscriptRecovery: (props: typeof dialogProps) => { dialogProps = props; return null; },
}));
mock.module('../../src/services/indexedDBService', () => ({
  indexedDBService: { deleteOldMeetings: deleteOld, deleteSavedMeetings: mock(async () => {}) },
}));

const { StartupChecks } = await import('../../src/app/_components/StartupChecks');

async function mount(): Promise<ReactTestRenderer> {
  let r!: ReactTestRenderer;
  await act(async () => { r = create(<StartupChecks />); });
  return r;
}
let mounted: ReactTestRenderer[] = [];
async function mountTracked() { const r = await mount(); mounted.push(r); return r; }

beforeEach(() => {
  for (const r of mounted) act(() => r.unmount());
  mounted = [];
  sessionStorage.clear();
  recording = { isRecording: false, status: originals.recording.RecordingStatus.IDLE };
  for (const m of [check, recoverMeeting, deleteOld, toast.error]) m.mockClear();
});

test('startup on / runs the recovery check and opens the dialog', async () => {
  await mountTracked();
  expect(deleteOld).toHaveBeenCalled();
  expect(check).toHaveBeenCalledTimes(1);
  expect(dialogProps.isOpen).toBe(true);
});

test('the recovery dialog is offered once per session', async () => {
  const first = await mountTracked();
  expect(dialogProps.isOpen).toBe(true);
  await act(async () => { first.unmount(); });
  mounted = [];
  await mountTracked();
  expect(check).toHaveBeenCalledTimes(2);
  expect(dialogProps.isOpen).toBe(false);
});

test('while recording, the startup check is skipped', async () => {
  recording = { isRecording: true, status: originals.recording.RecordingStatus.RECORDING };
  await mountTracked();
  expect(check).not.toHaveBeenCalled();
  expect(deleteOld).not.toHaveBeenCalled();
  expect(dialogProps.isOpen).toBe(false);
});

test('a rejected recovery shows the error toast', async () => {
  await mountTracked();
  let caught: unknown = null;
  await act(async () => { await dialogProps.onRecover('m1').catch((e) => { caught = e; }); });
  expect(caught).toBeInstanceOf(Error);
  expect(toast.error).toHaveBeenCalledWith('Failed to recover meeting', { description: 'disk full' });
});
