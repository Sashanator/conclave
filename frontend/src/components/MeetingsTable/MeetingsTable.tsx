'use client';

import type { ReactNode } from 'react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { ChevronDown } from 'lucide-react';
import { toast } from 'sonner';
import { flexRender, getCoreRowModel, useReactTable, type ColumnDef } from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useTemplates } from '@/hooks/meeting-details/useTemplates';
import { useMeetingRows } from '@/hooks/useMeetingRows';
import { useMeetingViews } from '@/hooks/useMeetingViews';
import { arrangeRows, defaultCollapsed, effectiveHidden, parseViewConfig, readViewUi, saveViewUi, type SortId, type ViewUi } from '@/lib/meeting-views';
import { rememberOrigin } from '@/lib/back-nav';
import { meetingHref } from '@/lib/meeting-target';
import { moveSelection, tableKeyAction } from '@/lib/meetings-keys';
import { createMeeting, meetingState, type MeetingRow } from '@/lib/meetings';
import { CategoryChip, DateCell, NameCell, ReviewedCell, SummaryCell } from './cells';
import { DeleteMeetingDialog } from './DeleteMeetingDialog';
import { RowMenu } from './RowMenu';
import { SearchResults, useMeetingSearch } from './SearchResults';
import { FilterChips, HeaderMenu, nextSort, SortIndicator, SORTABLE, Toolbar, TITLES } from './Toolbar';
import { ViewTabs } from './ViewTabs';

const VIRTUALIZE_ABOVE = 200;
const DEFAULT_WIDTH: Record<string, number> = { name: 280, date: 190, category: 150, summary: 420, reviewed: 90 };
// Date and Reviewed never shrink or stretch; the other columns share what is left.
const FIXED_PX: Record<string, number> = { date: 190, reviewed: 84 };
const errText = (e: unknown) => (e instanceof Error ? e.message : String(e));

type Item = { kind: 'group'; key: string; label: string; count: number; chip: string | null; collapsed: boolean } | { kind: 'row'; key: string; row: MeetingRow };

export function MeetingsTable({ onAsk, onScopeChange }: {
  onAsk?: () => void;
  onScopeChange?: (s: { viewName: string; meetingIds: string[] }) => void;
}) {
  const { views, active, config, dispatch, addView, renameView, removeView, setActive } = useMeetingViews();
  const { availableTemplates } = useTemplates();
  const { rows, loaded, error, reload, liveId, toggleReviewed, changeTemplate, removeMeeting } = useMeetingRows(availableTemplates);
  const { activeDuration } = useRecordingState();
  const [now, setNow] = useState(() => new Date());
  useEffect(() => { setNow(new Date()); }, [rows]); // every reload, so Today and Tomorrow roll over
  useEffect(() => { const t = setInterval(() => setNow(new Date()), 60_000); return () => clearInterval(t); }, []);
  const [deleting, setDeleting] = useState<MeetingRow | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const creating = useRef(false);
  const router = useRouter();
  const [query, setQuery] = useState('');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const found = useMeetingSearch(query);
  const searchMode = query.trim() !== '';
  const hits = found ? found.hits : undefined;

  const sections = useMemo(
    () => arrangeRows(rows, config, { liveId, now, templates: availableTemplates }),
    [rows, config, liveId, now, availableTemplates],
  );
  const data = useMemo(() => sections.flatMap((s) => s.rows), [sections]);
  // Groups the user flipped away from their default; a day that appears later gets its default.
  const [flipped, setFlipped] = useState<Set<string>>(() => new Set());
  const defaults = useMemo(() => defaultCollapsed(sections, config.group, now), [sections, config.group, now]);
  const scrollPos = useRef<ViewUi['scroll']>(null);
  const pendingScroll = useRef<ViewUi['scroll']>(null);
  const restoredSel = useRef<string | null>(null); // the selection a restore just applied, which must not scroll the list
  const [restored, setRestored] = useState(0); // lets the scroll effect run after the restored groups are applied
  const ui = useRef<ViewUi>({ flipped: [], query: '', selectedId: null, scroll: null });
  ui.current = { flipped: [...flipped], query, selectedId, scroll: null };
  // One effect per view: restore its state on entry, save it on exit (switching tabs or leaving the page).
  const activeId = active?.id;
  useEffect(() => {
    if (!activeId) return;
    const s = readViewUi(activeId);
    setFlipped(new Set(s?.flipped));
    setQuery(s?.query ?? '');
    setSelectedId(s?.selectedId ?? null);
    restoredSel.current = s?.selectedId ?? null;
    pendingScroll.current = s?.scroll ?? { px: 0, key: null }; // a view with nothing saved starts at the top
    setRestored((n) => n + 1);
    scrollPos.current = s?.scroll ?? null;
    return () => saveViewUi(activeId, { ...ui.current, scroll: scrollPos.current }); // scroll is a ref, so not in ui.current
  }, [activeId]);
  const toggleGroup = (key: string) => setFlipped((c) => { const n = new Set(c); if (!n.delete(key)) n.add(key); return n; });
  const items = useMemo<Item[]>(() => sections.flatMap((s) => {
    const closed = !!s.label && s.key !== 'now' && defaults.has(s.key) !== flipped.has(s.key);
    const i = config.group === 'category' ? availableTemplates.findIndex((t) => t.id === s.key) : -1;
    return [
      ...(s.label ? [{ kind: 'group' as const, key: `g-${s.key}`, label: s.label, count: s.rows.length, collapsed: closed,
        chip: config.group === 'category' ? `var(--cat-${i >= 0 ? (i % 4) + 1 : 5})` : null }] : []),
      ...(closed ? [] : s.rows.map((row) => ({ kind: 'row' as const, key: row.id, row }))),
    ];
  }), [sections, defaults, flipped, config.group, availableTemplates]);

  const counts = useMemo(() => Object.fromEntries(views.map((v) => [v.id,
    arrangeRows(rows, parseViewConfig(v.id, v.configJson), { liveId, now, templates: availableTemplates }).reduce((n, s) => n + s.rows.length, 0)])),
  [views, rows, liveId, now, availableTemplates]);

  const markOrigin = useCallback(() => rememberOrigin({ path: '/', label: `Meetings / ${active?.name ?? 'All meetings'}` }), [active?.name]);
  const newMeeting = useCallback(async () => {
    if (creating.current) return;
    creating.current = true;
    try {
      const id = await createMeeting();
      markOrigin();
      router.push(meetingHref({ id }));
    } catch (e) {
      toast.error('Could not create the meeting', { description: errText(e) });
    } finally {
      creating.current = false;
    }
  }, [router, markOrigin]);

  const ids = useMemo(() => (searchMode ? [...new Set((hits ?? []).map((h) => h.meetingId))] : items.flatMap((i) => (i.kind === 'row' ? [i.row.id] : []))), [searchMode, hits, items]);
  const selected = selectedId && ids.includes(selectedId) ? selectedId : null;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;
      if (document.querySelector('[role="dialog"]')) {
        if (tableKeyAction(e, { inEditable: true, searching: false }) === 'new-meeting') e.preventDefault(); // not a new WebView2 window
        return;
      }
      const inEditable = !!(e.target as Element | null)?.closest?.('input,textarea,select,button,a,[contenteditable="true"],[role="menuitem"],[role="option"]');
      const action = tableKeyAction(e, { inEditable, searching: query.trim() !== '' });
      if (!action) return;
      e.preventDefault(); // Ctrl N would open a window in WebView2; Esc must not also close the sidebar
      if (action === 'up' || action === 'down') setSelectedId(ids[moveSelection(selected ? ids.indexOf(selected) : -1, action === 'down' ? 1 : -1, ids.length)] ?? null);
      else if (action === 'open') { if (selected) { markOrigin(); router.push(meetingHref({ id: selected })); } }
      else if (action === 'toggle-reviewed') { if (selected && rows.find((r) => r.id === selected)?.hasSummary) void toggleReviewed(selected); }
      else if (action === 'focus-search') searchRef.current?.focus();
      else if (action === 'new-meeting') void newMeeting();
      else setQuery('');
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [ids, selected, query, rows, router, toggleReviewed, newMeeting, markOrigin]);

  useEffect(() => {
    onScopeChange?.({ viewName: active?.name ?? '', meetingIds: data.map((r) => r.id) });
  }, [data, active, onScopeChange]);

  const columns = useMemo<ColumnDef<MeetingRow>[]>(() => {
    const stateOf = (r: MeetingRow) => meetingState(r, liveId, now);
    const col = (id: string, cell: (r: MeetingRow) => ReactNode): ColumnDef<MeetingRow> =>
      ({ id, header: TITLES[id], size: DEFAULT_WIDTH[id], minSize: 60, cell: (c) => cell(c.row.original) });
    return [
      col('name', (r) => <NameCell row={r} onOpen={markOrigin} />),
      col('date', (r) => <DateCell row={r} now={now} state={stateOf(r)} />),
      col('category', (r) => <CategoryChip row={r} templates={availableTemplates} onChange={(t) => void changeTemplate(r.id, t)} />),
      col('summary', (r) => <SummaryCell row={r} state={stateOf(r)} activeDuration={activeDuration} compact={config.density === 'compact'} onDelete={() => setDeleting(r)} onOpen={markOrigin} />),
      col('reviewed', (r) => <ReviewedCell row={r} onToggle={() => void toggleReviewed(r.id)} />),
    ];
  }, [liveId, now, availableTemplates, activeDuration, config.density, changeTemplate, toggleReviewed, markOrigin]);

  const hidden = effectiveHidden(config);
  const table = useReactTable({
    data,
    columns,
    getCoreRowModel: getCoreRowModel(),
    getRowId: (r) => r.id,
    columnResizeMode: 'onEnd',
    state: {
      columnVisibility: Object.fromEntries(hidden.map((c) => [c, false])),
      columnSizing: config.widths,
    },
    onColumnSizingChange: (u) => {
      const next = typeof u === 'function' ? u(config.widths) : u;
      for (const [column, width] of Object.entries(next)) {
        if (config.widths[column] !== width) dispatch({ type: 'setWidth', column, width });
      }
    },
  });
  // Widths are proportions, so the columns always fit beside the AI sidebar instead of scrolling sideways.
  const sizeTotal = table.getVisibleLeafColumns().reduce((n, c) => (c.id in FIXED_PX ? n : n + c.getSize()), 0) || 1;

  const virtual = items.length > VIRTUALIZE_ABOVE;
  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => (config.density === 'compact' ? 32 : 40),
    overscan: 10,
    enabled: virtual,
  });
  const vitems = virtual ? virtualizer.getVirtualItems() : null;
  const shown = vitems ? vitems.map((v) => items[v.index]) : items;
  const padTop = vitems?.length ? vitems[0].start : 0;
  const padBottom = vitems?.length ? virtualizer.getTotalSize() - vitems[vitems.length - 1].end : 0;
  useEffect(() => {
    if (!selected) return;
    const fromRestore = restoredSel.current === selected;
    restoredSel.current = null;
    if (searchMode || pendingScroll.current || fromRestore) return; // a restore wins over scroll-to-selected
    if (virtual) { virtualizer.scrollToIndex(items.findIndex((i) => i.key === selected)); return; }
    Array.from(scrollRef.current?.querySelectorAll('[data-row-id]') ?? []).find((el) => el.getAttribute('data-row-id') === selected)
      ?.scrollIntoView?.({ block: 'nearest' });
  }, [selected]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    const p = pendingScroll.current;
    if (!p || !loaded || !items.length) return;
    pendingScroll.current = null;
    if (virtual && p.key) {
      const i = items.findIndex((x) => x.key === p.key);
      if (i >= 0) virtualizer.scrollToIndex(i, { align: 'start' });
    } else if (scrollRef.current) scrollRef.current.scrollTop = p.px;
  }, [loaded, items.length, virtual, restored]); // eslint-disable-line react-hooks/exhaustive-deps
  const colCount = table.getVisibleLeafColumns().length + 1;
  const rowById = new Map(table.getRowModel().rows.map((r) => [r.id, r]));

  if (!loaded && error) {
    return <p className="text-sm text-muted-foreground">Could not load meetings. <button type="button" className="underline" onClick={() => void reload()}>Try again</button></p>;
  }
  if (loaded && !rows.length) {
    return (
      <div className="mx-auto max-w-md py-16 text-center">
        <h2 className="text-lg font-semibold">No meetings yet</h2>
        <p className="mt-2 text-sm text-muted-foreground">Press Record new meeting, or create one now and write prep notes before the call. Calls marked 📞 in your calendar appear here by themselves.</p>
        <div className="mt-4 flex justify-center gap-2">
          <button type="button" onClick={() => void newMeeting()} className="h-8 rounded-md bg-primary px-3 text-sm font-semibold text-primary-foreground hover:opacity-90">New meeting (Ctrl N)</button>
          <Link href="/settings?tab=knowledge" className="inline-flex h-8 items-center rounded-md px-3 text-sm text-muted-foreground hover:bg-accent hover:text-foreground">Connect calendar</Link>
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ViewTabs views={views} activeId={active?.id ?? null} counts={counts} onSelect={setActive} onAdd={() => void addView()}
        onRename={(id, name) => void renameView(id, name)} onDelete={(id) => void removeView(id)} />
      <Toolbar onNew={() => void newMeeting()} query={query} onQuery={setQuery} searchRef={searchRef} config={config} dispatch={dispatch} onAsk={onAsk} />
      {!searchMode && <FilterChips filter={config.filter} now={now} templates={availableTemplates} dispatch={dispatch} />}
      {searchMode ? (
        <SearchResults query={query} hits={hits} rows={rows} templates={availableTemplates} now={now} selectedId={selected}
          onClear={() => setQuery('')} onToggleReviewed={(id) => void toggleReviewed(id)} onChangeTemplate={(id, t) => void changeTemplate(id, t)} onOpen={markOrigin} />
      ) : !loaded ? (
        <p role="status" className="py-8 text-center text-sm text-muted-foreground">Loading meetings…</p>
      ) : !data.length ? (
        <p className="py-8 text-center text-sm text-muted-foreground">No meetings match this view.</p>
      ) : (
      <div ref={scrollRef} onScroll={(e) => {
        const top = e.currentTarget.scrollTop;
        scrollPos.current = { px: top, key: virtual ? items[virtualizer.getVirtualItemForOffset(top)?.index ?? -1]?.key ?? null : null };
      }} className={`min-h-0 flex-1 overflow-auto ${config.density === 'compact' ? 'compact' : ''}`}>
        <table className="w-full table-fixed border-collapse text-sm">
          <thead>
            <tr>
              {table.getHeaderGroups()[0].headers.map((h) => (
                <th key={h.id} style={{ width: h.column.id in FIXED_PX ? `${FIXED_PX[h.column.id]}px` : `${(h.getSize() / sizeTotal) * 100}%` }} className="px-[10px] py-1 text-left text-xs font-semibold text-muted-foreground"
                  aria-sort={config.sort[0]?.id === h.column.id ? (config.sort[0].desc ? 'descending' : 'ascending') : undefined}>
                  {SORTABLE.includes(h.column.id as SortId)
                    ? <button type="button" onClick={() => dispatch(nextSort(config, h.column.id as SortId))} className="hover:text-foreground">
                        {flexRender(h.column.columnDef.header, h.getContext())}<SortIndicator config={config} id={h.column.id} />
                      </button>
                    : flexRender(h.column.columnDef.header, h.getContext())}
                  <HeaderMenu id={h.column.id} config={config} dispatch={dispatch} />
                  {!(h.column.id in FIXED_PX) && <span role="separator" aria-orientation="vertical" onMouseDown={h.getResizeHandler()} onTouchStart={h.getResizeHandler()}
                    className="float-right h-4 w-1 cursor-col-resize" />}
                </th>
              ))}
              <th style={{ width: 40 }}><span className="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {padTop > 0 && <tr aria-hidden style={{ height: padTop }}><td colSpan={colCount} /></tr>}
            {shown.map((item) => {
              if (item.kind === 'group') {
                const gk = item.key.slice(2);
                if (gk === 'now') return <tr key={item.key}><th colSpan={colCount} scope="colgroup" className="px-[10px] pt-3 text-left text-xs font-semibold text-foreground">{item.label}</th></tr>;
                return (
                  <tr key={item.key}>
                    <th colSpan={colCount} scope="colgroup" className="px-[10px] pt-3 text-left text-xs font-semibold text-muted-foreground">
                      <button type="button" aria-expanded={!item.collapsed} onClick={() => toggleGroup(gk)} className="inline-flex items-center gap-1.5 hover:text-foreground">
                        <ChevronDown className={`h-3.5 w-3.5 transition-transform ${item.collapsed ? '-rotate-90' : ''}`} aria-hidden />
                        {item.chip
                          ? <span className="rounded-full px-2 py-0.5 text-foreground" style={{ background: `hsl(${item.chip} / .15)` }}>{item.label}</span>
                          : <span className="text-foreground">{item.label}</span>}
                        <span className="font-normal">{item.count}</span>
                      </button>
                    </th>
                  </tr>
                );
              }
              const row = rowById.get(item.row.id);
              if (!row) return null;
              return (
                <tr key={item.key} data-row-id={item.row.id} aria-current={selected === item.row.id || undefined} onClick={() => setSelectedId(item.row.id)}
                  className={`border-b ${selected === item.row.id ? 'bg-accent' : 'hover:bg-accent'}`} style={{ height: 'var(--row)' }}>
                  {row.getVisibleCells().map((cell) => (
                    <td key={cell.id} className={`overflow-hidden text-ellipsis px-[10px] ${config.density === 'compact' ? 'whitespace-nowrap' : ''}`}>
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </td>
                  ))}
                  <td><RowMenu row={item.row} live={item.row.id === liveId} onDelete={() => setDeleting(item.row)} onOpen={markOrigin} /></td>
                </tr>
              );
            })}
            {padBottom > 0 && <tr aria-hidden style={{ height: padBottom }}><td colSpan={colCount} /></tr>}
            <tr><td colSpan={colCount}><button type="button" onClick={() => void newMeeting()} className="px-[10px] py-2 text-sm text-muted-foreground hover:text-foreground">+ New meeting</button></td></tr>
          </tbody>
        </table>
      </div>
      )}
      <DeleteMeetingDialog
        row={deleting}
        open={deleting !== null}
        onOpenChange={(o) => { if (!o) setDeleting(null); }}
        // A failed delete keeps the row and the hook toasts the error.
        onConfirm={async () => { const r = deleting; setDeleting(null); if (r) await removeMeeting(r.id); }}
      />
    </div>
  );
}
