import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  navigation: { ...await import('next/navigation') },
  rec: { ...await import('../../src/contexts/RecordingStateContext') },
  provider: { ...await import('../../src/components/Sidebar/SidebarProvider') },
  imp: { ...await import('../../src/contexts/ImportDialogContext') },
  feed: { ...await import('../../src/components/Feed/FeedBell') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('next/navigation', () => originals.navigation);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.rec);
  mock.module('../../src/components/Sidebar/SidebarProvider', () => originals.provider);
  mock.module('../../src/contexts/ImportDialogContext', () => originals.imp);
  mock.module('../../src/components/Feed/FeedBell', () => originals.feed);
});

const push = mock((_u: string) => {});
const openImportDialog = mock(() => {});
const invoke = mock(async (cmd: string, _a?: unknown): Promise<unknown> => {
  if (cmd === 'plugin:app|version') return '1.3.0';
  if (cmd === 'api_route_recording_start') return { meetingId: 'm9', title: 'T' };
  return null;
});
let collapsed = false;
const toggleCollapsed = mock(() => {});
let rec: Record<string, unknown> = {};
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));
mock.module('next/navigation', () => ({ usePathname: () => '/tasks', useRouter: () => ({ push, replace() {} }) }));
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals.rec, useRecordingState: () => rec }));
mock.module('../../src/components/Sidebar/SidebarProvider', () => ({
  ...originals.provider,
  useSidebar: () => ({ collapsed, toggleCollapsed }),
}));
mock.module('../../src/contexts/ImportDialogContext', () => ({ ...originals.imp, useImportDialog: () => ({ openImportDialog }) }));
mock.module('../../src/components/Feed/FeedBell', () => ({ FeedBell: () => null }));

const Sidebar = (await import('../../src/components/Sidebar')).default;
const { setUnsavedChanges } = await import('../../src/lib/unsaved-guard');

async function render(): Promise<ReactTestRenderer> {
  let r!: ReactTestRenderer;
  await act(async () => { r = create(<Sidebar />); });
  return r;
}
const text = (n: any): string => (n.children ?? []).map((c: any) => (typeof c === 'string' ? c : text(c))).join('');
const buttons = (r: ReactTestRenderer) => r.root.findAllByType('button').filter((b) => b.props['aria-label'] !== 'Collapse sidebar' && b.props['aria-label'] !== 'Expand sidebar');

beforeEach(() => {
  collapsed = false;
  rec = { isRecording: false, liveMeetingId: null, activeDuration: null };
  push.mockClear(); invoke.mockClear(); openImportDialog.mockClear();
  sessionStorage.clear(); localStorage.clear();
});

describe('Sidebar nav', () => {
  test('expanded labels in order', async () => {
    const r = await render();
    expect(buttons(r).map((b) => text(b).replace(' (new)', '').trim())).toEqual([
      'Record new meeting', 'Meetings', 'Tasks', 'Digests', 'Dictate', 'Practice', 'Stats', 'Import audio', 'Settings', "What's new in v1.3.0",
    ]);
    expect(r.root.findByProps({ 'aria-label': 'Main' }).type).toBe('nav');
  });

  test('collapsed: aria-labels, no dropped entries, active page', async () => {
    collapsed = true;
    const r = await render();
    const labels = buttons(r).map((b) => b.props['aria-label']);
    for (const l of ['Meetings', 'Tasks', 'Digests', 'Dictate', 'Practice', 'Stats', 'Import audio', 'Settings']) expect(labels).toContain(l);
    for (const l of ['Ask', 'Prep', 'About', 'Briefs & digests']) expect(labels).not.toContain(l);
    expect(buttons(r).find((b) => b.props['aria-label'] === 'Tasks')!.props['aria-current']).toBe('page');
  });

  test('live recording shows the clock and opens the meeting', async () => {
    rec = { isRecording: true, liveMeetingId: 'm1', activeDuration: 192 };
    const r = await render();
    const b = buttons(r)[0];
    expect(text(b)).toContain('Recording 03:12');
    await act(async () => { b.props.onClick(); });
    expect(push).toHaveBeenCalledWith('/meeting-details?id=m1');
  });

  test('a recording with no meeting row can still be opened', async () => {
    rec = { isRecording: true, liveMeetingId: null, activeDuration: 5 };
    const r = await render();
    const b = buttons(r)[0];
    expect(b.props.disabled).toBeFalsy();
    await act(async () => { b.props.onClick(); });
    expect(push).toHaveBeenCalledWith('/meeting-details');
  });

  test('idle click routes the start and opens the meeting page', async () => {
    const r = await render();
    await act(async () => { await buttons(r)[0].props.onClick(); });
    expect(invoke).toHaveBeenCalledWith('api_route_recording_start', { source: 'sidebar' });
    expect(push.mock.calls[0][0]).toMatch(/^\/meeting-details\?id=m9&start=\d+$/);
    expect(sessionStorage.getItem('autoStartRecording')).toBe('true');
    expect(sessionStorage.getItem('autoStartMeetingId')).toBe('m9');
  });

  test('a declined leave routes nothing and queues nothing', async () => {
    setUnsavedChanges(true);
    const confirm = mock(() => false);
    (globalThis as any).window.confirm = confirm;
    try {
      const r = await render();
      await act(async () => { await buttons(r)[0].props.onClick(); });
      expect(confirm).toHaveBeenCalled();
      expect(invoke).not.toHaveBeenCalledWith('api_route_recording_start', expect.anything());
      expect(push).not.toHaveBeenCalled();
      expect(sessionStorage.getItem('autoStartRecording')).toBeNull();
    } finally {
      setUnsavedChanges(false);
    }
  });

  test('a routing failure still records, as a new meeting on the id-less page', async () => {
    const before = invoke.getMockImplementation()!;
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'api_route_recording_start') throw 'db locked';
      return before(cmd);
    });
    try {
      const r = await render();
      await act(async () => { await buttons(r)[0].props.onClick(); });
      expect(push).toHaveBeenCalledWith('/meeting-details');
      expect(sessionStorage.getItem('autoStartRecording')).toBe('true');
    } finally {
      invoke.mockImplementation(before);
    }
  });

  test('import opens the dialog', async () => {
    const r = await render();
    await act(async () => { buttons(r).find((b) => text(b).includes('Import audio'))!.props.onClick(); });
    expect(openImportDialog).toHaveBeenCalled();
  });

  test('the new dot goes after whats-new-seen', async () => {
    const r = await render();
    expect(JSON.stringify(r.toJSON())).toContain('(new)');
    await act(async () => { localStorage.setItem('whats-new-seen', '1.3.0'); window.dispatchEvent(new Event('whats-new-seen')); });
    expect(JSON.stringify(r.toJSON())).not.toContain('(new)');
  });
});
