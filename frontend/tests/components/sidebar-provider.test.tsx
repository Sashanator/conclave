import { afterAll, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  rec: { ...await import('../../src/contexts/RecordingStateContext') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.rec);
});

const invoke = mock(async (_cmd: string, _a?: unknown): Promise<unknown> => null);
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));
mock.module('@tauri-apps/api/event', () => ({ ...originals.event, listen: async () => () => {} }));
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals.rec, useRecordingState: () => ({ isRecording: false }) }));

const { SidebarProvider, useSidebar } = await import('../../src/components/Sidebar/SidebarProvider');

test('the provider keeps only collapse, server address and summary polling, and never lists meetings', async () => {
  let ctx: Record<string, unknown> = {};
  const Probe = () => { ctx = useSidebar() as unknown as Record<string, unknown>; return null; };
  await act(async () => { create(<SidebarProvider><Probe /></SidebarProvider>); });
  expect(invoke.mock.calls.some(([cmd]) => cmd === 'api_get_meetings')).toBe(false);
  expect(Object.keys(ctx).sort()).toEqual(['isCollapsed', 'serverAddress', 'startSummaryPolling', 'stopSummaryPolling', 'toggleCollapse']);
});
