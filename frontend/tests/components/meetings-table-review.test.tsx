import { afterAll, afterEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originalCore = { ...await import('@tauri-apps/api/core') };
const originalSonner = { ...await import('sonner') };
const originalMenu = { ...await import('../../src/components/ui/dropdown-menu') };
const originalPopover = { ...await import('../../src/components/ui/popover') };
const originalCommand = { ...await import('../../src/components/ui/command') };

let invokeImpl: () => Promise<unknown> = async () => null;
const toastError = mock((_m: string, _o?: unknown) => {});
mock.module('@tauri-apps/api/core', () => ({ ...originalCore, invoke: () => invokeImpl() }));
mock.module('sonner', () => ({ toast: { error: toastError, warning: () => {} } }));
const pass = ({ children }: { children?: ReactNode }) => <>{children}</>;
// Menus render their content inline; items are buttons (select = click, checkbox = toggle).
mock.module('../../src/components/ui/dropdown-menu', () => ({
  ...originalMenu,
  DropdownMenu: pass, DropdownMenuTrigger: pass, DropdownMenuContent: pass, DropdownMenuLabel: pass, DropdownMenuSeparator: pass,
  DropdownMenuRadioGroup: pass, DropdownMenuRadioItem: pass,
  DropdownMenuItem: ({ children, onSelect }: { children?: ReactNode; onSelect?: () => void }) => <button type="button" data-item onClick={onSelect}>{children}</button>,
  DropdownMenuCheckboxItem: ({ children, onCheckedChange }: { children?: ReactNode; onCheckedChange?: () => void }) => <button type="button" data-item onClick={onCheckedChange}>{children}</button>,
}));
mock.module('../../src/components/ui/popover', () => ({
  Popover: ({ open, onOpenChange, children }: { open: boolean; onOpenChange: (o: boolean) => void; children?: ReactNode }) => (
    <div data-popover data-open={String(open)} onClick={() => onOpenChange(true)}>{children}</div>
  ),
  PopoverTrigger: pass, PopoverContent: pass,
}));
mock.module('../../src/components/ui/command', () => ({
  Command: pass, CommandInput: () => null, CommandList: pass, CommandEmpty: pass,
  CommandItem: ({ children, onSelect }: { children?: ReactNode; onSelect?: () => void }) => <button type="button" data-cmd onClick={onSelect}>{children}</button>,
}));
const { ViewTabs } = await import('../../src/components/MeetingsTable/ViewTabs');
const { FilterChips } = await import('../../src/components/MeetingsTable/Toolbar');
const { CategoryChip } = await import('../../src/components/MeetingsTable/cells');
const { useTemplates } = await import('../../src/hooks/meeting-details/useTemplates');
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originalCore);
  mock.module('sonner', () => originalSonner);
  mock.module('../../src/components/ui/dropdown-menu', () => originalMenu);
  mock.module('../../src/components/ui/popover', () => originalPopover);
  mock.module('../../src/components/ui/command', () => originalCommand);
});

let renderer: ReactTestRenderer | undefined;
afterEach(async () => { if (renderer) await act(async () => renderer!.unmount()); renderer = undefined; });
const text = (n: ReactTestInstance): string => n.children.map((c) => (typeof c === 'string' ? c : text(c))).join('');
const btn = (label: string) => renderer!.root.findAll((n) => n.type === 'button' && text(n) === label)[0];
async function render(el: ReactNode) { await act(async () => { renderer = create(el as never); }); }

describe('view rename', () => {
  test('Escape is stopped and prevented, so it neither clears the search nor closes the AI sidebar, and saves nothing', async () => {
    const onRename = mock((_id: string, _n: string) => {});
    await render(<ViewTabs views={[{ id: 'v1', name: 'Mine', position: 0, builtin: false, configJson: '{}' }]} activeId="v1" counts={{}}
      onSelect={() => {}} onAdd={() => {}} onRename={onRename} onDelete={() => {}} />);
    await act(async () => { btn('Rename').props.onClick(); });
    const input = renderer!.root.findByProps({ 'aria-label': 'View name' });
    const preventDefault = mock(() => {}); const stopPropagation = mock(() => {});
    await act(async () => { input.props.onKeyDown({ key: 'Escape', currentTarget: { value: 'X' }, preventDefault, stopPropagation }); });
    expect(preventDefault).toHaveBeenCalled();
    expect(stopPropagation).toHaveBeenCalled();
    expect(onRename).not.toHaveBeenCalled();
    expect(renderer!.root.findAllByProps({ 'aria-label': 'View name' }).length).toBe(0);
  });
});

describe('filters', () => {
  const now = new Date('2026-11-17T12:00:00Z');
  const run = async (filter = {}) => {
    const dispatch = mock((_a: unknown) => {});
    await render(<FilterChips filter={filter} now={now} templates={[]} dispatch={dispatch} />);
    return dispatch;
  };

  test('a Status checkbox filters by that state, and unchecking the last one drops the filter', async () => {
    let dispatch = await run();
    await act(async () => { btn('Finished').props.onClick(); });
    expect(dispatch).toHaveBeenCalledWith({ type: 'setFilter', filter: { states: ['finished'] } });
    dispatch = await run({ states: ['finished'] });
    await act(async () => { btn('Finished').props.onClick(); });
    expect(dispatch.mock.calls[0][0]).toEqual({ type: 'setFilter', filter: { states: undefined } });
  });

  test('the Status chip shows readable names', async () => {
    await run({ states: ['not-recorded', 'new'] });
    expect(text(renderer!.root)).toContain('Status: Not recorded, New');
  });

  test('a custom date range is applied, in order', async () => {
    const dispatch = await run();
    await act(async () => { btn('Date: Custom range…').props.onClick(); });
    const form = renderer!.root.findByType('form');
    const values: Record<string, string> = { from: '2026-11-20', to: '2026-11-10' };
    const g = globalThis as unknown as { FormData: unknown };
    const real = g.FormData;
    g.FormData = class { get(k: string) { return values[k]; } };
    try { await act(async () => { form.props.onSubmit({ preventDefault() {}, currentTarget: {} }); }); } finally { g.FormData = real; }
    expect(dispatch).toHaveBeenCalledWith({ type: 'setFilter', filter: { range: { from: '2026-11-10', to: '2026-11-20' } } });
    expect(renderer!.root.findAllByType('form').length).toBe(0);
  });
});

describe('category popover', () => {
  test('selecting a template changes it and closes the popover', async () => {
    const onChange = mock((_t: string | null) => {});
    const row = { id: 'm', title: 'M', templateId: null } as never;
    await render(<CategoryChip row={row} templates={[{ id: 't1', name: 'Interview' }]} onChange={onChange} />);
    const pop = () => renderer!.root.findByProps({ 'data-popover': true });
    await act(async () => { pop().props.onClick(); });
    expect(pop().props['data-open']).toBe('true');
    await act(async () => { btn('Interview').props.onClick(); });
    expect(onChange).toHaveBeenCalledWith('t1');
    expect(pop().props['data-open']).toBe('false');
  });
});

describe('useTemplates', () => {
  test('a failed template list toasts', async () => {
    toastError.mockClear();
    invokeImpl = async () => { throw new Error('nope'); };
    function T() { useTemplates(); return null; }
    await act(async () => { renderer = create(<T />); });
    await act(async () => { await new Promise((r) => setTimeout(r, 0)); });
    expect(toastError).toHaveBeenCalledWith('Could not load templates', { description: 'nope' });
  });
});
