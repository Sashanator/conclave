'use client';

import Link from 'next/link';
import { useRef } from 'react';
import { toast } from 'sonner';
import { prepCounter, tickItem, type MeetingPrep } from '@/lib/prep';

export function PrepTickList({ prep, onChange }: { prep: MeetingPrep; onChange(p: MeetingPrep): void }) {
  const href = `/prep?${new URLSearchParams({ event: prep.eventUid, start: prep.eventStart })}`;
  // Each tick's reply is the whole list. Replies can resolve out of order, so only a reply newer than the one shown is applied.
  const issued = useRef(0);
  const applied = useRef(0);

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
                  const seq = ++issued.current;
                  tickItem(prep.id, item.id, !item.ticked)
                    .then((next) => {
                      if (seq > applied.current) { applied.current = seq; onChange(next); }
                    })
                    .catch((error) => toast.error(`Failed to save Prep tick: ${String(error)}`));
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
