'use client';

import { useState } from 'react';
import Link from 'next/link';
import { Command, CommandEmpty, CommandInput, CommandItem, CommandList } from '@/components/ui/command';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { clock, meetingHref } from '@/lib/meeting-target';
import { formatMeetingDate, type MeetingRow, type MeetingState } from '@/lib/meetings';
import type { CategoryInfo } from '@/lib/meeting-views';

const muted = 'text-muted-foreground';

export function NameCell({ row, onOpen }: { row: MeetingRow; onOpen: () => void }) {
  return (
    <Link href={meetingHref({ id: row.id })} onClick={onOpen} className="hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
      {row.emoji ? <span aria-hidden>{row.emoji} </span> : null}{row.title}
    </Link>
  );
}

export const DateCell = ({ row, now, state }: { row: MeetingRow; now: Date; state: MeetingState }) =>
  <span className={muted}>{formatMeetingDate(row, now, state === 'recording')}</span>;

export function SummaryCell({ row, state, activeDuration, compact = false, onDelete, onOpen }: {
  row: MeetingRow; state: MeetingState; activeDuration: number | null; compact?: boolean; onDelete: () => void; onOpen: () => void;
}) {
  switch (state) {
    case 'recording':
      return <span className="rounded-full px-2 text-xs font-semibold text-white" style={{ background: 'hsl(var(--rec))' }}>
        {`Recording · ${clock(activeDuration ?? 0)}`}
      </span>;
    case 'new':
      return <span className={`flex items-center gap-2 ${muted}`}>
        <span>Not recorded yet</span>
        {row.calendarUid && <span className="rounded-full bg-muted px-2 text-xs">Calendar</span>}
        {row.prepPoints > 0 && <span>{`Prep: ${row.prepPoints} point${row.prepPoints === 1 ? '' : 's'}`}</span>}
      </span>;
    case 'not-recorded':
      return <span className={`flex items-center gap-2 ${muted}`}>
        <span>Not recorded</span>
        <Link href={meetingHref({ id: row.id, tab: 'prep' })} onClick={onOpen} className="text-foreground underline">Open prep</Link>
      </span>;
    case 'cancelled':
      return <span className={`flex items-center gap-2 ${muted}`}>
        <span>Cancelled in calendar</span>
        {row.prepPoints > 0 && <span>· has prep notes, so it stays</span>}
        <button type="button" onClick={onDelete} className="text-foreground underline">Delete</button>
      </span>;
    default:
      return <span className={compact ? 'block truncate' : 'line-clamp-2'}>{row.gist}</span>;
  }
}

export function ReviewedCell({ row, onToggle }: { row: MeetingRow; onToggle: () => void }) {
  if (!row.hasSummary) return null;
  return <input type="checkbox" checked={row.reviewed} onChange={onToggle} aria-label={`Reviewed: ${row.title}`} />;
}

export function CategoryChip({ row, templates, onChange }: {
  row: MeetingRow; templates: CategoryInfo[]; onChange: (templateId: string | null) => void;
}) {
  const i = templates.findIndex((t) => t.id === row.templateId);
  const label = i >= 0 ? templates[i].name : 'No template';
  const color = `var(--cat-${i >= 0 ? (i % 4) + 1 : 5})`;
  const [open, setOpen] = useState(false);
  const pick = (id: string | null) => { setOpen(false); onChange(id); };
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button type="button" aria-label={`Template: ${label}`}
          className="rounded-full px-2 text-xs" style={{ background: `hsl(${color} / .12)` }}>
          {label}
        </button>
      </PopoverTrigger>
      <PopoverContent className="w-56 p-0" align="start">
        <Command>
          <CommandInput placeholder="Find a template" />
          <CommandList>
            <CommandEmpty>No match</CommandEmpty>
            <CommandItem onSelect={() => pick(null)}>No template</CommandItem>
            {templates.map((t) => (
              <CommandItem key={t.id} value={t.name} onSelect={() => pick(t.id)}>{t.name}</CommandItem>
            ))}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
