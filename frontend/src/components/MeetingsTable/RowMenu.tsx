'use client';

import Link from 'next/link';
import { MoreHorizontal } from 'lucide-react';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { meetingHref } from '@/lib/meeting-target';
import type { MeetingRow } from '@/lib/meetings';

export function RowMenu({ row, live, onDelete }: { row: MeetingRow; live: boolean; onDelete: () => void }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button type="button" aria-label={`Actions: ${row.title}`} className="rounded p-1 text-muted-foreground hover:bg-accent">
          <MoreHorizontal className="h-4 w-4" aria-hidden />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem asChild><Link href={meetingHref({ id: row.id })}>Open</Link></DropdownMenuItem>
        <DropdownMenuItem disabled={live} onSelect={onDelete}>Delete meeting…</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
