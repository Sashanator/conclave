import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  navigation: { ...await import('next/navigation') },
  rec: { ...await import('../../src/contexts/RecordingStateContext') },
  provider: { ...await import('../../src/components/Sidebar/SidebarProvider') },
  imp: { ...await import('../../src/contexts/ImportDialogContext') },
  feed: { ...await import('../../src/components/Feed/FeedBell') },
  confirm: { ...await import('../../src/lib/confirm') },
  event: { ...await import('@tauri-apps/api/event') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('next/navigation', () => originals.navigation);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.rec);
  mock.module('../../src/components/Sidebar/SidebarProvider', () => originals.provider);
  mock.module('../../src/contexts/ImportDialogContext', () => originals.imp);
  mock.module('../../src/components/Feed/FeedBell', () => originals.feed);
  mock.module('../../src/lib/confirm', () => originals.confirm);
});

const push = mock((_u: string) => {});
const openImportDialog = mock(() => {});
const invoke = mock(async (cmd: string, _a?: unknown): Promise<unknown> => {
  if (cmd === 'plugin:app|version') return '1.3.0';
  if (cmd === 'api_route_recording_start') return { meetingId: 'm9', title: 'T' };
  if (cmd === 'tasks_queue_list') return queue;
  return null;
});
let queue: unknown = null;
let tasksChanged: (() => void) | null = null;
mock.module('@tauri-apps/api/event', () => ({
  ...originals.event,
  listen: async (event: string, handler: () => void) => {
    if (event === 'tasks-changed') tasksChanged = handler;
    return () => {};
  },
}));
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
const confirmAction = mock(async (o: any): Promise<boolean> => originals.confirm.confirmAction(o));
mock.module('../../src/lib/confirm', () => ({ ...originals.confirm, confirmAction }));

const { TooltipContent } = await import('../../src/components/ui/tooltip');
const Sidebar = (await import('../../src/components/Sidebar')).default;
const { setUnsavedChanges } = await import('../../src/lib/unsaved-guard');

async function render(): Promise<ReactTestRenderer> {
  let r!: ReactTestRenderer;
  await act(async () => { r = create(<Sidebar />); });
  return r;
}
const text = (n: any): string => (n.children ?? []).map((c: any) => (typeof c === 'string' ? c : text(c))).join('');
const buttons = (r: ReactTestRenderer) => r.root.findAllByType('button').filter((b) => !String(b.props['aria-label']).startsWith('Collapse sidebar') && b.props['aria-label'] !== 'Expand sidebar');

beforeEach(() => {
  collapsed = false;
  rec = { isRecording: false, liveMeetingId: null, activeDuration: null };
  confirmAction.mockClear(); push.mockClear(); invoke.mockClear(); openImportDialog.mockClear();
  sessionStorage.clear(); localStorage.clear();
  queue = null; tasksChanged = null;
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

  test('a dirty guard and a stay answer keep the route', async () => {
    setUnsavedChanges(true);
    try {
      const r = await render();
      confirmAction.mockImplementationOnce(async () => false);
      await act(async () => { await buttons(r)[1].props.onClick(); });
      expect(confirmAction).toHaveBeenCalledTimes(1);
      expect(push).not.toHaveBeenCalled();
      confirmAction.mockImplementationOnce(async () => true);
      await act(async () => { await buttons(r)[1].props.onClick(); });
      expect(push).toHaveBeenCalledTimes(1);
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

  test('no floating toggle', async () => {
    const r = await render();
    const json = JSON.stringify(r.toJSON());
    expect(json).not.toContain('-right-6');
    expect(json).not.toContain('rounded-full shadow-lg');
  });

  test('the collapse row is the last nav row', async () => {
    toggleCollapsed.mockClear();
    const r = await render();
    const all = r.root.findByProps({ 'aria-label': 'Main' }).findAllByType('button');
    const last = all[all.length - 1];
    expect(last.props['aria-label']).toBe('Collapse sidebar Ctrl B');
    expect(text(last)).toContain('Collapse sidebar');
    expect(text(last)).toContain('Ctrl B');
    await act(async () => { last.props.onClick(); });
    expect(toggleCollapsed).toHaveBeenCalled();
  });

  test('collapsed shows the expand row with the shortcut in its tooltip', async () => {
    collapsed = true;
    const r = await render();
    const all = r.root.findByProps({ 'aria-label': 'Main' }).findAllByType('button');
    expect(all[all.length - 1].props['aria-label']).toBe('Expand sidebar');
    const tips = r.root.findAllByType(TooltipContent).map((t) => t.props.children.props.children);
    expect(tips).toContain('Expand sidebar (Ctrl B)');
  });

  test('the live recording item is red', async () => {
    let r = await render();
    expect(buttons(r)[0].props.className).toContain('bg-primary');
    rec = { isRecording: true, liveMeetingId: 'm1', activeDuration: 192 };
    r = await render();
    const cls = buttons(r)[0].props.className as string;
    expect(cls).toContain('bg-[hsl(var(--rec))]');
    expect(cls).toContain('text-white');
    expect(cls).not.toContain('bg-primary');
  });

  test('Tasks badge counts the To review tasks and refreshes on tasks-changed', async () => {
    queue = [{ review: 'open' }, { review: 'keep' }, { review: 'sent' }, { review: 'drop' }];
    const r = await render();
    expect(buttons(r).map((b) => b.props['aria-label'])).toContain('Tasks, 2 to review');
    queue = [];
    await act(async () => { tasksChanged!(); await new Promise((res) => setTimeout(res, 0)); });
    expect(buttons(r).map((b) => b.props['aria-label'])).toContain('Tasks');
    expect(buttons(r).map((b) => b.props['aria-label'])).not.toContain('Tasks, 2 to review');
  });

  test('collapsed recording shows the clock in the rail', async () => {
    collapsed = true;
    rec = { isRecording: true, liveMeetingId: 'm1', activeDuration: 192 };
    const r = await render();
    expect(text(buttons(r)[0])).toContain('03:12');
  });
});
