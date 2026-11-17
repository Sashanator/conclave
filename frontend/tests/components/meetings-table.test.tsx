import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingRow } from '../../src/lib/meetings';

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
});

const mk = (id: string, p: Partial<MeetingRow> = {}): MeetingRow => ({
  id, title: id, titleSource: 'user', templateId: null, emoji: null, reviewed: false, gist: `gist ${id}`,
  startedAt: null, endedAt: null, createdAt: '2026-11-16 10:00:00+00:00', calendarUid: null,
  calendarStart: null, calendarEnd: null, calendarCancelledAt: null, hasTranscript: false, hasSummary: false, prepPoints: 0, ...p,
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

let deleteImpl: () => Promise<unknown> = async () => null;
const invoke = mock(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
  switch (command) {
    case 'api_list_meeting_rows': return fixture;
    case 'api_list_meeting_views': return [{ id: 'all', name: 'All meetings', position: 0, builtin: true, configJson: '{}' }];
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
mock.module('../../src/components/Sidebar/SidebarProvider', () => ({ ...originals.sidebar, useSidebar: () => ({ refetchMeetings: async () => {} }) }));
mock.module('../../src/components/ui/dialog', () => ({
  Dialog: ({ open, children }: { open: boolean; children?: ReactNode }) => (open ? <>{children}</> : null),
  DialogContent: pass, DialogDescription: pass, DialogFooter: pass, DialogHeader: pass, DialogTitle: pass,
}));
mock.module('../../src/components/ui/dropdown-menu', () => ({
  DropdownMenu: pass, DropdownMenuTrigger: pass, DropdownMenuContent: () => null, DropdownMenuItem: pass,
}));
mock.module('../../src/components/ui/popover', () => ({ Popover: pass, PopoverTrigger: pass, PopoverContent: () => null }));
mock.module('../../src/components/ui/command', () => ({
  Command: pass, CommandInput: () => null, CommandList: pass, CommandEmpty: pass, CommandItem: pass,
}));

const { MeetingsTable } = await import('../../src/components/MeetingsTable');

const text = (n: ReactTestInstance): string => n.children.map((c) => (typeof c === 'string' ? c : text(c))).join('');
const all = () => text(renderer!.root);
const button = (label: string) => renderer!.root.findAll((n) => n.type === 'button' && text(n) === label)[0];
const deleteCalls = () => invoke.mock.calls.filter((c) => c[0] === 'api_delete_meeting');
async function flush() { await act(async () => { await new Promise((r) => setTimeout(r, 0)); }); }

describe('meetings table', () => {
  beforeEach(async () => {
    invoke.mockClear();
    toastError.mockClear();
    deleteImpl = async () => null;
    if (renderer) await act(async () => { renderer!.unmount(); });
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
});
