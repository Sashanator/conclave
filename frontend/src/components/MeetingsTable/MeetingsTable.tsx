'use client';

import type { ReactNode } from 'react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { flexRender, getCoreRowModel, useReactTable, type ColumnDef } from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useTemplates } from '@/hooks/meeting-details/useTemplates';
import { useMeetingRows } from '@/hooks/useMeetingRows';
import { useMeetingViews } from '@/hooks/useMeetingViews';
import { arrangeRows, effectiveHidden } from '@/lib/meeting-views';
import { meetingState, type MeetingRow } from '@/lib/meetings';
import { CategoryChip, DateCell, NameCell, ReviewedCell, SummaryCell } from './cells';
import { DeleteMeetingDialog } from './DeleteMeetingDialog';
import { RowMenu } from './RowMenu';

const VIRTUALIZE_ABOVE = 200;
const DEFAULT_WIDTH: Record<string, number> = { name: 280, date: 190, category: 150, summary: 420, reviewed: 90 };
const TITLES: Record<string, string> = { name: 'Name', date: 'Date', category: 'Category', summary: 'Summary', reviewed: 'Reviewed' };

type Item = { kind: 'group'; key: string; label: string } | { kind: 'row'; key: string; row: MeetingRow };

export function MeetingsTable({ onScopeChange }: {
  onAsk?: () => void; // wired by the toolbar (Task 24)
  onScopeChange?: (s: { viewName: string; meetingIds: string[] }) => void;
}) {
  const { rows, loaded, error, reload, liveId, toggleReviewed, changeTemplate, removeMeeting } = useMeetingRows();
  const { active, config, dispatch } = useMeetingViews();
  const { availableTemplates } = useTemplates();
  const { activeDuration } = useRecordingState() as { activeDuration: number | null };
  const [now] = useState(() => new Date());
  const [deleting, setDeleting] = useState<MeetingRow | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  const sections = useMemo(
    () => arrangeRows(rows, config, { liveId, now, templates: availableTemplates }),
    [rows, config, liveId, now, availableTemplates],
  );
  const data = useMemo(() => sections.flatMap((s) => s.rows), [sections]);
  const items = useMemo<Item[]>(() => sections.flatMap((s) => [
    ...(s.label ? [{ kind: 'group' as const, key: `g-${s.key}`, label: s.label }] : []),
    ...s.rows.map((row) => ({ kind: 'row' as const, key: row.id, row })),
  ]), [sections]);

  useEffect(() => {
    onScopeChange?.({ viewName: active?.name ?? '', meetingIds: data.map((r) => r.id) });
  }, [data, active, onScopeChange]);

  const columns = useMemo<ColumnDef<MeetingRow>[]>(() => {
    const stateOf = (r: MeetingRow) => meetingState(r, liveId, now);
    const col = (id: string, cell: (r: MeetingRow) => ReactNode): ColumnDef<MeetingRow> =>
      ({ id, header: TITLES[id], size: DEFAULT_WIDTH[id], minSize: 60, cell: (c) => cell(c.row.original) });
    return [
      col('name', (r) => <NameCell row={r} />),
      col('date', (r) => <DateCell row={r} now={now} state={stateOf(r)} />),
      col('category', (r) => <CategoryChip row={r} templates={availableTemplates} onChange={(t) => void changeTemplate(r.id, t)} />),
      col('summary', (r) => <SummaryCell row={r} state={stateOf(r)} activeDuration={activeDuration} onDelete={() => setDeleting(r)} />),
      col('reviewed', (r) => <ReviewedCell row={r} onToggle={() => void toggleReviewed(r.id)} />),
    ];
  }, [liveId, now, availableTemplates, activeDuration, changeTemplate, toggleReviewed]);

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
  const colCount = table.getVisibleLeafColumns().length + 1;
  const rowById = new Map(table.getRowModel().rows.map((r) => [r.id, r]));

  if (!loaded && error) {
    return <p className="text-sm text-muted-foreground">Could not load meetings. <button type="button" className="underline" onClick={() => void reload()}>Try again</button></p>;
  }
  if (loaded && !data.length) return <p className="text-sm text-muted-foreground">No meetings here yet.</p>;

  return (
    <>
      <div ref={scrollRef} className="overflow-auto" style={{ ['--row' as string]: config.density === 'compact' ? '32px' : '40px' }}>
        <table className="w-full table-fixed border-collapse text-sm">
          <thead>
            <tr>
              {table.getHeaderGroups()[0].headers.map((h) => (
                <th key={h.id} style={{ width: h.getSize() }} className="px-[10px] py-1 text-left text-xs font-semibold text-muted-foreground">
                  {flexRender(h.column.columnDef.header, h.getContext())}
                  <span role="separator" aria-orientation="vertical" onMouseDown={h.getResizeHandler()} onTouchStart={h.getResizeHandler()}
                    className="float-right h-4 w-1 cursor-col-resize" />
                </th>
              ))}
              <th style={{ width: 40 }}><span className="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {padTop > 0 && <tr aria-hidden style={{ height: padTop }}><td colSpan={colCount} /></tr>}
            {shown.map((item) => {
              if (item.kind === 'group') {
                return <tr key={item.key}><th colSpan={colCount} scope="colgroup" className="px-[10px] pt-3 text-left text-xs font-semibold text-muted-foreground">{item.label}</th></tr>;
              }
              const row = rowById.get(item.row.id);
              if (!row) return null;
              return (
                <tr key={item.key} className="border-b hover:bg-accent" style={{ height: 'var(--row)' }}>
                  {row.getVisibleCells().map((cell) => (
                    <td key={cell.id} className="overflow-hidden text-ellipsis px-[10px]">
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </td>
                  ))}
                  <td><RowMenu row={item.row} live={item.row.id === liveId} onDelete={() => setDeleting(item.row)} /></td>
                </tr>
              );
            })}
            {padBottom > 0 && <tr aria-hidden style={{ height: padBottom }}><td colSpan={colCount} /></tr>}
          </tbody>
        </table>
      </div>
      <DeleteMeetingDialog
        row={deleting}
        open={deleting !== null}
        onOpenChange={(o) => { if (!o) setDeleting(null); }}
        // A failed delete keeps the row and the hook toasts the error.
        onConfirm={async () => { const r = deleting; setDeleting(null); if (r) await removeMeeting(r.id); }}
      />
    </>
  );
}
