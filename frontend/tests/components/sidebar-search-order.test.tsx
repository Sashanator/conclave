import { afterAll, afterEach, describe, expect, mock, spyOn, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { SearchHit } from '../../src/lib/meeting-target';

const originals = {
  event: { ...await import('@tauri-apps/api/event') },
  core: { ...await import('@tauri-apps/api/core') },
  navigation: { ...await import('next/navigation') },
  recordingState: { ...await import('../../src/contexts/RecordingStateContext') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('next/navigation', () => originals.navigation);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.recordingState);
});

type Deferred = { promise: Promise<unknown>; resolve: (value: unknown) => void };
function deferred(): Deferred {
  let resolve!: (value: unknown) => void;
  const promise = new Promise<unknown>((r) => { resolve = r; });
  return { promise, resolve };
}

// Searches answer in the order they were issued; each test queues the responses it controls.
const pendingSearches: Deferred[] = [];
const invoke = mock(async (command: string): Promise<unknown> => {
  if (command === 'continue_status') return { offer: null, target: null };
  if (command === 'api_search_transcripts') return pendingSearches.shift()!.promise;
  return [];
});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('next/navigation', () => ({ usePathname: () => '/meeting-details', useRouter: () => ({ push() {}, replace() {} }) }));
mock.module('../../src/contexts/RecordingStateContext', () => ({
  ...originals.recordingState,
  useRecordingState: () => ({ isRecording: false }),
}));
const handlers = new Map<string, Set<(event: { payload: unknown }) => void>>();
mock.module('@tauri-apps/api/event', () => ({
  ...originals.event,
  listen: async (name: string, handler: (event: { payload: unknown }) => void) => {
    const listeners = handlers.get(name) ?? new Set();
    handlers.set(name, listeners);
    listeners.add(handler);
    return () => { listeners.delete(handler); };
  },
}));
const { toast } = await import('sonner');
const { ContinueControl } = await import('../../src/app/_components/ContinueControl');
const { SidebarProvider, useSidebar } = await import('../../src/components/Sidebar/SidebarProvider');

type Sidebar = ReturnType<typeof useSidebar>;
let sidebar: Sidebar;
function Probe() {
  sidebar = useSidebar();
  return null;
}

let renderer: ReactTestRenderer | undefined;
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  pendingSearches.length = 0;
});

async function mount() {
  await act(async () => { renderer = create(<SidebarProvider><Probe /></SidebarProvider>); });
}

const older = [{ meetingId: 'old' }] as SearchHit[];
const newer = [{ meetingId: 'new' }] as SearchHit[];

describe('transcript search responses', () => {
  test('a slow earlier response does not overwrite the results of a newer search', async () => {
    await mount();
    const first = deferred();
    const second = deferred();
    pendingSearches.push(first, second);
    await act(async () => {
      const a = sidebar.searchTranscripts('a');
      const b = sidebar.searchTranscripts('ab');
      second.resolve(newer);
      await b;
      first.resolve(older);
      await a;
    });
    expect(sidebar.searchResults).toEqual(newer);
  });

  test('an earlier response arriving after the query was cleared does not bring results back', async () => {
    await mount();
    const first = deferred();
    pendingSearches.push(first);
    await act(async () => {
      const a = sidebar.searchTranscripts('a');
      await sidebar.searchTranscripts('');
      first.resolve(older);
      await a;
    });
    expect(sidebar.searchResults).toEqual([]);
    expect(sidebar.isSearching).toBe(false);
  });

  test('searching stays on while the newest search is still pending', async () => {
    await mount();
    const first = deferred();
    const second = deferred();
    pendingSearches.push(first, second);
    let newest!: Promise<void>;
    await act(async () => {
      const a = sidebar.searchTranscripts('a');
      newest = sidebar.searchTranscripts('ab');
      first.resolve(older);
      await a;
    });
    expect(sidebar.isSearching).toBe(true);
    await act(async () => {
      second.resolve(newer);
      await newest;
    });
    expect(sidebar.isSearching).toBe(false);
  });
});

describe('app-wide continue failure toast', () => {
  test.each([false, true])('one toast with ContinueControl mounted: %s', async (withControl) => {
    const error = spyOn(toast, 'error').mockImplementation(() => 'toast');
    try {
      await act(async () => {
        renderer = create(<SidebarProvider>{withControl ? <ContinueControl /> : <Probe />}</SidebarProvider>);
      });
      const listeners = handlers.get('continue-target-failed');
      expect(listeners?.size).toBe(1);
      await act(async () => { listeners?.forEach((handler) => handler({ payload: { error: 'Target unavailable.' } })); });
      expect(error).toHaveBeenCalledTimes(1);
      expect(error).toHaveBeenCalledWith('Could not continue the earlier meeting', {
        description: 'Target unavailable. This recording will be saved as a new meeting.',
      });
      await act(async () => { renderer!.unmount(); });
      renderer = undefined;
      expect(listeners?.size).toBe(0);
    } finally {
      error.mockRestore();
    }
  });
});
