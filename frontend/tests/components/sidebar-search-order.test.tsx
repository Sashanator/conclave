import { afterAll, afterEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { SearchHit } from '../../src/lib/meeting-target';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  navigation: { ...await import('next/navigation') },
  recordingState: { ...await import('../../src/contexts/RecordingStateContext') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
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
  if (command === 'api_search_transcripts') return pendingSearches.shift()!.promise;
  return [];
});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('next/navigation', () => ({ usePathname: () => '/', useRouter: () => ({ push() {}, replace() {} }) }));
mock.module('../../src/contexts/RecordingStateContext', () => ({
  ...originals.recordingState,
  useRecordingState: () => ({ isRecording: false }),
}));
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
