'use client';

import { Button } from '@/components/ui/button';
import { sendButtonLabel, type SendSummary } from '@/lib/tasks-queue';

export type SendResult = { sent: string | null; notSent: string | null; error?: string };

export function SendBar(p: { summary: SendSummary; sending: boolean; result: SendResult | null; onSend(): void; onView(): void }) {
  const { summary: s, result: r } = p;
  const n = s.sendableIds.length;
  const amber = 'text-amber-700 dark:text-amber-400';
  return (
    <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 border-t bg-card px-4 py-3 text-sm">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <span><b>{s.kept}</b> kept</span>
        {s.byDest.map((d) => <span key={d.dest.id}>{d.dest.name} <b>{d.n}</b></span>)}
        {s.needDb > 0 && <span className={amber}>{s.needDb} need a database</span>}
        {s.waiting > 0 && <span className={amber}>{s.waiting} waiting for {s.waitingNames.join(', ')}</span>}
        {s.undecided > 0 && <span className="text-muted-foreground">{s.undecided} not decided</span>}
      </div>
      <div className="flex flex-wrap items-center gap-3">
        <span role="status" aria-live="polite" className="text-xs">
          {r?.error && <span className="text-red-600 dark:text-red-400">{r.error}</span>}
          {r?.sent && <span className="text-emerald-700 dark:text-emerald-400">{r.sent} <button type="button" className="underline" onClick={p.onView}>View</button></span>}
          {r?.notSent && <span className={`${amber} ${r.sent ? 'ml-2' : ''}`}>{r.notSent}</span>}
        </span>
        <Button type="button" disabled={p.sending || n === 0} onClick={p.onSend}>{sendButtonLabel(n, p.sending)}</Button>
      </div>
    </div>
  );
}
