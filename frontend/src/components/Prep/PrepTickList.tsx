'use client';

import Link from 'next/link';
import { useRef } from 'react';
import { toast } from 'sonner';
import { prepCounter, tickItem, type MeetingPrep } from '@/lib/prep';

export function PrepTickList({ prep, onChange }: { prep: MeetingPrep; onChange(p: MeetingPrep): void }) {
  const href = `/prep?${new URLSearchParams({ event: prep.eventUid, start: prep.eventStart })}`;
  const latest = useRef(prep);
  latest.current = prep;
  const queue = useRef<Promise<void>>(Promise.resolve());

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-3">
      <div className="flex items-center gap-2 text-sm">
        <span>{prepCounter(prep.items)}</span>
        {prep.approvedAt == null && <span className="text-xs text-muted-foreground">draft</span>}
      </div>
      <ul className="space-y-3">
        {prep.items.map((item) => (
          <li key={item.id}>
            <label className="flex items-start gap-2 text-sm">
              <input
                type="checkbox"
                checked={item.ticked}
                onChange={() => {
                  queue.current = queue.current.then(async () => {
                    const current = latest.current;
                    const target = current.items.find(i => i.id === item.id);
                    if (current.id !== prep.id || !target) return;
                    try {
                      const next = await tickItem(current.id, target.id, !target.ticked);
                      latest.current = next;
                      onChange(next);
                    } catch (error) { toast.error(`Failed to save Prep tick: ${String(error)}`); }
                  });
                }}
                className="mt-1"
              />
              <span className="min-w-0 break-words">
                {item.text}
                <span className="ml-2 rounded bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">{item.kind}</span>
              </span>
            </label>
          </li>
        ))}
      </ul>
      <Link href={href} className="text-sm text-primary hover:underline">Open in Prep</Link>
    </div>
  );
}
