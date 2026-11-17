import { afterAll, beforeEach, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';

const originals = {
  event: { ...await import('@tauri-apps/api/event') },
  nav: { ...await import('next/navigation') },
  rec: { ...await import('../../src/contexts/RecordingStateContext') },
  ai: { ...await import('../../src/components/AiSidebar/AiSidebarProvider') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('next/navigation', () => originals.nav);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.rec);
  mock.module('../../src/components/AiSidebar/AiSidebarProvider', () => originals.ai);
});

const push = mock((_u: string) => {});
let handler: (() => void) | undefined;
let rec: Record<string, unknown> = {};
mock.module('@tauri-apps/api/event', () => ({ ...originals.event, listen: async (_n: string, h: () => void) => { handler = h; return () => {}; } }));
mock.module('next/navigation', () => ({ useRouter: () => ({ push }) }));
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals.rec, useRecordingState: () => rec }));
mock.module('../../src/components/AiSidebar/AiSidebarProvider', () => ({ ...originals.ai, useAiSidebar: () => ({ kind: 'table', open: false, toggle() {} }) }));

const { CopilotOpenListener } = await import('../../src/components/AiSidebar/AiSidebar');

beforeEach(() => { push.mockClear(); handler = undefined; localStorage.clear(); });

async function fire(state: Record<string, unknown>) {
  rec = state;
  await act(async () => { create(<CopilotOpenListener />); });
  await act(async () => { handler!(); });
}

test('opens the live meeting', async () => {
  await fire({ isRecording: true, liveMeetingId: 'm1' });
  expect(push).toHaveBeenCalledWith('/meeting-details?id=m1');
});

test('a recording with no meeting row opens the id-less page', async () => {
  await fire({ isRecording: true, liveMeetingId: null });
  expect(push).toHaveBeenCalledWith('/meeting-details');
});

test('no recording goes nowhere', async () => {
  await fire({ isRecording: false, liveMeetingId: null });
  expect(push).not.toHaveBeenCalled();
});
