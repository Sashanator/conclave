import { afterAll, beforeEach, describe, expect, mock, setSystemTime, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingRow } from '../../src/lib/meetings';
import { BUILTIN_VIEW_DEFAULTS, clearViewUi } from '../../src/lib/meeting-views';

let renderer: ReactTestRenderer | null = null;
const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  sonner: { ...await import('sonner') },
  rec: { ...await import('../../src/contexts/RecordingStateContext') },
  sidebar: { ...await import('../../src/components/Sidebar/SidebarProvider') },
  dialog: { ...await import('../../src/components/ui/dialog') },
  menu: { ...await import('../../src/components/ui/dropdown-menu') },
  popover: { ...await import('../../src/components/ui/popover') },
  command: { ...await import('../../src/components/ui/command') },
  nav: { ...await import('next/navigation') },
};
afterAll(() => {
  // A mounted table would re-render against the restored real context and throw into later files.
  renderer?.unmount();
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.rec);
  mock.module('../../src/components/Sidebar/SidebarProvider', () => originals.sidebar);
  mock.module('../../src/components/ui/dialog', () => originals.dialog);
  mock.module('../../src/components/ui/dropdown-menu', () => originals.menu);
  mock.module('../../src/components/ui/popover', () => originals.popover);
  mock.module('../../src/components/ui/command', () => originals.command);
  mock.module('next/navigation', () => originals.nav);
  try { localStorage.clear(); } catch { /* none */ }
});

const mk = (id: string, p: Partial<MeetingRow> = {}): MeetingRow => ({
  id, title: id, titleSource: 'user', templateId: null, emoji: null, reviewed: false, gist: `gist ${id}`,
  startedAt: null, endedAt: null, createdAt: '2026-11-16 10:00:00+00:00', calendarUid: null,
  calendarStart: null, calendarEnd: null, calendarCancelledAt: null, hasTranscript: false, hasSummary: false, prepPoints: 0, hasPrep: false, ...p,
});
const soon = new Date(Date.now() + 3600_000).toISOString();
const fixture = [
  mk('done1', { hasTranscript: true, hasSummary: true, startedAt: '2026-11-16 10:00:00+00:00' }),
  mk('done2', { hasTranscript: true, hasSummary: true, reviewed: true, startedAt: '2026-11-15 10:00:00+00:00' }),
  mk('nosum', { hasTranscript: true, startedAt: '2026-11-14 10:00:00+00:00' }),
  mk('fresh', { calendarUid: 'u1', calendarStart: soon, prepPoints: 3 }),
  mk('missed', { calendarStart: '2020-01-01T09:00:00+00:00', createdAt: '2020-01-01 09:00:00+00:00' }),
  mk('cancelled', { calendarCancelledAt: '2026-11-10T00:00:00Z', calendarStart: soon, prepPoints: 2 }),
];

let allCfg = '{}';
let rowsImpl: () => MeetingRow[] = () => fixture;
let searchImpl: () => unknown[] = () => [];
const push = mock((_href: string) => {});
let deleteImpl: () => Promise<unknown> = async () => null;
const invoke = mock(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
  switch (command) {
    case 'api_list_meeting_rows': return rowsImpl();
    case 'api_list_meeting_views': return [
      { id: 'all', name: 'All meetings', position: 0, builtin: true, configJson: allCfg },
      { id: 'week', name: 'This week', position: 1, builtin: true, configJson: JSON.stringify(BUILTIN_VIEW_DEFAULTS.week) },
    ];
    case 'api_create_meeting': return 'preview-new';
    case 'api_search_transcripts': return searchImpl();
    case 'api_list_templates': return { templates: [], errors: [] };
    case 'api_delete_meeting': return deleteImpl();
    default: return null;
  }
});
const toastError = mock((_m: string, _o?: unknown) => {});
const pass = ({ children }: { children?: ReactNode }) => <>{children}</>;
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));
mock.module('@tauri-apps/api/event', () => ({ ...originals.event, listen: async () => () => {} }));
mock.module('sonner', () => ({ toast: { error: toastError, warning: () => {} } }));
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals.rec, useRecordingState: () => ({ liveMeetingId: null, activeDuration: null }) }));
mock.module('../../src/components/Sidebar/SidebarProvider', () => ({ ...originals.sidebar, useSidebar: () => ({}) }));
mock.module('../../src/components/ui/dialog', () => ({
  Dialog: ({ open, children }: { open: boolean; children?: ReactNode }) => (open ? <>{children}</> : null),
  DialogContent: pass, DialogDescription: pass, DialogFooter: pass, DialogHeader: pass, DialogTitle: pass,
}));
mock.module('next/navigation', () => ({ ...originals.nav, useRouter: () => ({ push }) }));
mock.module('../../src/components/ui/dropdown-menu', () => ({
  DropdownMenu: pass, DropdownMenuTrigger: pass, DropdownMenuContent: () => null, DropdownMenuItem: pass,
  DropdownMenuCheckboxItem: pass, DropdownMenuLabel: pass, DropdownMenuRadioGroup: pass, DropdownMenuRadioItem: pass, DropdownMenuSeparator: pass,
}));
mock.module('../../src/components/ui/popover', () => ({ Popover: pass, PopoverTrigger: pass, PopoverContent: () => null }));
mock.module('../../src/components/ui/command', () => ({
  Command: pass, CommandInput: () => null, CommandList: pass, CommandEmpty: pass, CommandItem: pass,
}));

const { MeetingsTable, DeleteMeetingDialog } = await import('../../src/components/MeetingsTable');

const text = (n: ReactTestInstance): string => n.children.map((c) => (typeof c === 'string' ? c : text(c))).join('');
const all = () => text(renderer!.root);
const button = (label: string) => renderer!.root.findAll((n) => n.type === 'button' && text(n) === label)[0];
const deleteCalls = () => invoke.mock.calls.filter((c) => c[0] === 'api_delete_meeting');
async function flush() { await act(async () => { await new Promise((r) => setTimeout(r, 0)); }); }

describe('meetings table', () => {
  beforeEach(async () => {
    invoke.mockClear();
    toastError.mockClear();
    push.mockClear();
    allCfg = '{}';
    rowsImpl = () => fixture;
    searchImpl = () => [];
    setSystemTime();
    try { localStorage.clear(); } catch { /* none */ }
    deleteImpl = async () => null;
    if (renderer) await act(async () => { renderer!.unmount(); });
    clearViewUi(); // after the unmount, which saves the old view's state
    await act(async () => { renderer = create(<MeetingsTable />); });
    await flush();
  });

  test('renders each state', () => {
    for (const s of ['Not recorded yet', 'Prep: 3 points', 'Calendar', 'Cancelled in calendar', 'Not recorded', 'Open prep']) {
      expect(all()).toContain(s);
    }
  });

  test('one checkbox per row with a summary', () => {
    const boxes = renderer!.root.findAll((n) => n.type === 'input' && n.props.type === 'checkbox');
    expect(boxes.length).toBe(fixture.filter((r) => r.hasSummary).length);
  });

  test('delete asks first, then deletes once; cancel deletes nothing', async () => {
    await act(async () => { button('Delete').props.onClick(); });
    expect(all()).toContain('Delete “cancelled”?');
    expect(all()).toContain('This removes: prep (2 points), notes. Tasks you kept stay in Tasks.');
    expect(deleteCalls().length).toBe(0);
    await act(async () => { button('Cancel').props.onClick(); });
    expect(deleteCalls().length).toBe(0);
    await act(async () => { button('Delete').props.onClick(); });
    await act(async () => { button('Delete meeting').props.onClick(); });
    await flush();
    expect(deleteCalls().length).toBe(1);
    expect(deleteCalls()[0][1]).toEqual({ meetingId: 'cancelled' });
  });

  test('a failed delete keeps the row and toasts', async () => {
    deleteImpl = async () => { throw new Error('disk busy'); };
    await act(async () => { button('Delete').props.onClick(); });
    await act(async () => { button('Delete meeting').props.onClick(); });
    await flush();
    expect(toastError).toHaveBeenCalledWith('Could not delete the meeting', { description: 'disk busy' });
    expect(all()).toContain('Cancelled in calendar');
  });

  const mount = async (options?: Parameters<typeof create>[1]) => {
    if (renderer) await act(async () => { renderer!.unmount(); });
    await act(async () => { renderer = create(<MeetingsTable onScopeChange={scope} />, options); });
    await flush();
  };
  const search = async (q: string) => {
    await act(async () => { renderer!.root.findByProps({ 'aria-label': 'Search meetings, summaries and transcripts' }).props.onChange({ target: { value: q } }); });
    await act(async () => { await new Promise((r) => setTimeout(r, 300)); });
  };
  const key = (k: string, init: KeyboardEventInit = {}) => {
    const e = new KeyboardEvent('keydown', { key: k, cancelable: true, bubbles: true, ...init });
    act(() => { document.dispatchEvent(e); });
    return e;
  };
  const scope = mock((_s: { viewName: string; meetingIds: string[] }) => {});

  test('tabs, counts and header sort', async () => {
    await mount();
    expect(renderer!.root.findAll((n) => n.props.role === 'tab').length).toBe(2);
    expect(all()).toContain('This week');
    await act(async () => { button('Name').props.onClick(); });
    expect(renderer!.root.findAll((n) => n.type === 'th' && n.props['aria-sort'] === 'ascending').length).toBe(1);
  });

  test('no rows shows the first-run state; New meeting creates and opens it', async () => {
    rowsImpl = () => [];
    await mount();
    expect(all()).toContain('No meetings yet');
    expect(all()).toContain('Connect calendar');
    await act(async () => { button('New meeting (Ctrl N)').props.onClick(); });
    await flush();
    expect(invoke.mock.calls.some((c) => c[0] === 'api_create_meeting')).toBe(true);
    expect(push).toHaveBeenCalledWith('/meeting-details?id=preview-new');
  });

  test('a search with no hits says so and Clear search restores the rows', async () => {
    await search('zzz');
    expect(all()).toContain('Nothing mentions “zzz”. Try fewer words or another spelling.');
    expect(all()).not.toContain('Cancelled in calendar');
    await act(async () => { button('Clear search').props.onClick(); });
    expect(all()).toContain('Cancelled in calendar');
  });

  test('a failed search toasts and says so', async () => {
    searchImpl = () => { throw new Error('boom'); };
    await search('plan');
    expect(toastError.mock.calls.some((c) => c[0] === 'Search failed')).toBe(true);
    expect(all()).toContain('The search failed');
    expect(all()).not.toContain('Searching all meetings');
  });

  test('day grouping opens today only; a click flips one group', async () => {
    setSystemTime(new Date('2026-11-16T12:00:00Z'));
    allCfg = JSON.stringify({ group: 'day' });
    await mount();
    const toggles = (v: boolean) => renderer!.root.findAll((n) => n.type === 'button' && n.props['aria-expanded'] === v);
    expect(toggles(true).length).toBe(1);
    expect(text(toggles(true)[0])).toContain('Today · Monday 16 Nov');
    const sunday = toggles(false).find((t) => text(t).includes('Sunday 15 Nov'))!;
    expect(text(sunday)).toContain('1');
    expect(all()).not.toContain('done2');
    await act(async () => { sunday.props.onClick(); });
    expect(all()).toContain('done2');
    expect(toggles(true).length).toBe(2);
    setSystemTime();
  });

  test('category grouping starts with every group open', async () => {
    allCfg = '{}';
    await mount();
    expect(renderer!.root.findAll((n) => n.type === 'button' && n.props['aria-expanded'] === false).length).toBe(0);
  });

  test('scroll comes back after a remount', async () => {
    const nodeMock = (node: object) => ({ createNodeMock: (el: { type: unknown }) => (el.type === 'div' ? node : null) });
    await mount(nodeMock({ scrollTop: 0, querySelectorAll: () => [] }));
    await act(async () => { renderer!.root.findAll((n) => n.type === 'div' && n.props.onScroll)[0].props.onScroll({ currentTarget: { scrollTop: 480 } }); });
    const fresh = { scrollTop: 0, querySelectorAll: () => [] };
    await mount(nodeMock(fresh));
    expect(fresh.scrollTop).toBe(480);
  });

  describe('per-view memory', () => {
    const dayCfg = () => { setSystemTime(new Date('2026-11-16T12:00:00Z')); allCfg = JSON.stringify({ group: 'day' }); };
    const sundayBtn = () => renderer!.root.findAll((n) => n.type === 'button' && n.props['aria-expanded'] !== undefined && text(n).includes('Sunday 15 Nov'))[0];
    const row = (id: string) => renderer!.root.findAll((n) => n.type === 'tr' && n.props['data-row-id'] === id)[0];
    const searchInput = () => renderer!.root.findByProps({ 'aria-label': 'Search meetings, summaries and transcripts' });
    const tab = (name: string) => renderer!.root.findAll((n) => n.props.role === 'tab' && text(n).includes(name))[0];

    test('coming back restores groups, search text and the selected row', async () => {
      dayCfg();
      await mount();
      await act(async () => { sundayBtn().props.onClick(); });
      await act(async () => { row('done1').props.onClick(); });
      await mount();
      expect(all()).toContain('done2');
      expect(row('done1').props['aria-current']).toBe(true);
      await search('plan');
      await mount();
      expect(searchInput().props.value).toBe('plan');
      setSystemTime();
    });

    test('each view keeps its own state', async () => {
      dayCfg();
      await mount();
      await act(async () => { sundayBtn().props.onClick(); });
      expect(all()).toContain('done2');
      await act(async () => { tab('This week').props.onClick(); });
      await flush();
      expect(all()).not.toContain('Sunday 15 Nov');
      await act(async () => { tab('All meetings').props.onClick(); });
      await flush();
      expect(all()).toContain('done2');
      setSystemTime();
    });

    test('a fresh session starts from the defaults', async () => {
      dayCfg();
      await mount();
      await act(async () => { sundayBtn().props.onClick(); });
      await search('plan');
      await act(async () => { renderer!.unmount(); }); // saves on the way out
      clearViewUi();
      await mount();
      expect(all()).not.toContain('done2');
      expect(searchInput().props.value).toBe('');
      setSystemTime();
    });
  });

  test('typing a new query never shows the old hits, and shows Searching… instead of the table', async () => {
    searchImpl = () => [{ meetingId: 'done1', title: 'done1', source: 'transcript', snippet: 'old snippet', transcriptId: 't1', audioStart: 0, totalInMeeting: 1 }];
    await search('plan');
    expect(all()).toContain('old snippet');
    await act(async () => { renderer!.root.findByProps({ 'aria-label': 'Search meetings, summaries and transcripts' }).props.onChange({ target: { value: 'plan b' } }); });
    expect(all()).not.toContain('old snippet');
    expect(all()).toContain('Searching…');
    expect(all()).not.toContain('Cancelled in calendar');
  });

  test('before the first load there is a loading state, not "No meetings match"', async () => {
    rowsImpl = () => new Promise(() => {}) as never;
    await mount();
    expect(all()).toContain('Loading meetings…');
    expect(all()).not.toContain('No meetings match');
  });

  test('Ctrl N is prevented, and creates nothing, while a dialog is open', async () => {
    const dlg = document.createElement('div');
    dlg.setAttribute('role', 'dialog');
    document.body.appendChild(dlg);
    try {
      const e = key('n', { ctrlKey: true });
      await flush();
      expect(e.defaultPrevented).toBe(true);
      expect(push).not.toHaveBeenCalled();
    } finally { dlg.remove(); }
  });

  test('the table ends with a New meeting row', async () => {
    await act(async () => { button('+ New meeting').props.onClick(); });
    await flush();
    expect(push).toHaveBeenCalledWith('/meeting-details?id=preview-new');
  });

  test('search lists snippets and the rest of the matches', async () => {
    const hit = (i: number) => ({ meetingId: 'done1', title: 'done1', source: 'transcript', snippet: `snippet ${i}`, transcriptId: `t${i}`, audioStart: i * 60, totalInMeeting: 5 });
    searchImpl = () => [hit(1), hit(2)];
    await search('plan');
    expect(all()).toContain('Searching all meetings · 1 match');
    expect(all()).toContain('snippet 1');
    expect(all()).toContain('3 more in this meeting');
  });

  test('the week view reports its meeting ids', async () => {
    setSystemTime(new Date('2026-11-17T12:00:00Z'));
    localStorage.setItem('conclave.meetingsView', 'week');
    scope.mockClear();
    await mount();
    const last = scope.mock.calls[scope.mock.calls.length - 1][0];
    expect(last.viewName).toBe('This week');
    expect(last.meetingIds).toContain('done1');
    for (const id of ['done2', 'nosum', 'missed']) expect(last.meetingIds).not.toContain(id);
    setSystemTime();
  });

  test('Escape while searching is defaultPrevented and clears', async () => {
    await search('zzz');
    const e = key('Escape');
    expect(e.defaultPrevented).toBe(true);
    expect(renderer!.root.findByProps({ 'aria-label': 'Search meetings, summaries and transcripts' }).props.value).toBe('');
  });

  test('Space toggles Reviewed only on rows with a summary', async () => {
    const row = (id: string) => renderer!.root.findAll((n) => n.type === 'tr' && n.props['data-row-id'] === id)[0];
    const calls = () => invoke.mock.calls.filter((c) => c[0] === 'api_set_meeting_reviewed');
    await act(async () => { row('nosum').props.onClick(); });
    key(' ');
    await flush();
    expect(calls().length).toBe(0);
    await act(async () => { row('done1').props.onClick(); });
    key(' ');
    await flush();
    expect(calls().length).toBe(1);
  });

  test('Ctrl N is prevented and creates a meeting', async () => {
    const e = key('n', { ctrlKey: true });
    await flush();
    expect(e.defaultPrevented).toBe(true);
    expect(push).toHaveBeenCalledWith('/meeting-details?id=preview-new');
  });

  test('the delete dialog names the brief of a calendar meeting', async () => {
    let r: ReactTestRenderer;
    await act(async () => { r = create(<DeleteMeetingDialog row={mk('cal', { calendarUid: 'u9' })} open onOpenChange={() => {}} onConfirm={() => {}} />); });
    expect(text(r!.root)).toContain('This removes: notes, brief.');
    await act(async () => { r.unmount(); });
  });
});
