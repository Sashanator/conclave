'use client';

import { BLOCKING_STATES, type Destination } from '@/lib/notion';
import type { TaskRow } from '@/lib/tasks';

/** The task's Notion database: a coloured dot and a native select. Unset or deleted shows "Choose database…"; a blocked database turns amber. */
export function DestPicker({ task, dests, disabled, onPick }: { task: TaskRow; dests: Destination[]; disabled?: boolean; onPick(destId: string | null): void }) {
  const current = dests.find((d) => d.id === task.destId) ?? null;
  const bad = !!current?.lastStatus && BLOCKING_STATES.includes(current.lastStatus);
  const tone = !current ? 'border-dashed border-muted-foreground/50 text-muted-foreground pl-3' : bad ? 'border-amber-600 text-amber-700 dark:border-amber-400 dark:text-amber-400 pl-6' : 'border-border pl-6';
  return (
    <span className="relative inline-block">
      {current && <i aria-hidden="true" className="pointer-events-none absolute left-2.5 top-1/2 h-2 w-2 -translate-y-1/2 rounded-full ring-1 ring-border" style={{ background: `hsl(var(--db-${current.colour}))` }} />}
      <select
        aria-label={`Notion database for: ${task.text}`}
        disabled={disabled}
        value={current?.id ?? ''}
        onChange={(e) => onPick(e.target.value || null)}
        className={`min-w-[170px] rounded-full border bg-background py-0.5 pr-3 text-xs ${tone}`}
      >
        {!current && <option value="">Choose database…</option>}
        {[...dests].sort((a, b) => a.position - b.position).map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}
      </select>
    </span>
  );
}
