'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ChevronDown } from 'lucide-react';
import { formatUsd, type MeetingCost } from '@/lib/stats';

/** refreshKey (the summary status) refetches the cost after a summary is generated. */
export function MeetingCostLine({ meetingId, refreshKey }: { meetingId: string; refreshKey?: string }) {
  const [cost, setCost] = useState<MeetingCost | null>(null);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    let live = true;
    setCost(null);
    invoke<MeetingCost | null>('meeting_cost', { meetingId })
      .then((c) => {
        if (live) setCost(c);
      })
      .catch((err) => {
        console.error('Meeting cost failed to load:', err);
      });
    return () => {
      live = false;
    };
  }, [meetingId, refreshKey]);

  if (!cost || cost.by_purpose.length === 0) return null;

  return (
    <div className="mt-2 text-sm text-muted-foreground">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className="flex items-center gap-1 hover:text-foreground"
      >
        <span>{`LLM cost: ${formatUsd(cost.total_usd)} · ${(cost.tokens / 1000).toFixed(1)}k tokens`}</span>
        <ChevronDown className={`h-3 w-3 transition-transform ${open ? 'rotate-180' : ''}`} />
      </button>
      {open && (
        <ul className="mt-1 space-y-0.5 pl-1">
          {cost.by_purpose.map((p) => (
            <li key={p.purpose}>{`${p.purpose} · ${(p.tokens / 1000).toFixed(1)}k tokens · ${formatUsd(p.cost_usd)}`}</li>
          ))}
        </ul>
      )}
    </div>
  );
}
