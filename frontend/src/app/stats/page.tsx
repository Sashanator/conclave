'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { CostSection } from '@/components/Stats/CostSection';
import { TalkTimeSection } from '@/components/Stats/TalkTimeSection';
import { readStatsPeriod, saveStatsPeriod, type CostPeriod, type Period, type TalkTime } from '@/lib/stats';

// The current period plus the previous 8 weeks or 12 months
const COUNT: Record<Period, number> = { week: 9, month: 13 };

export default function StatsPage() {
  const [period, setPeriod] = useState<Period>('week');
  const [data, setData] = useState<{ cost: CostPeriod[]; talk: TalkTime } | null>(null);
  const [failed, setFailed] = useState(false);

  // Read after mount: the page is prerendered, where localStorage does not exist
  useEffect(() => {
    setPeriod(readStatsPeriod());
  }, []);

  useEffect(() => {
    let live = true;
    setData(null);
    setFailed(false);
    Promise.all([
      invoke<CostPeriod[]>('stats_cost', { period, count: COUNT[period] }),
      invoke<TalkTime>('stats_talk_time', { period }),
    ])
      .then(([cost, talk]) => {
        if (live) setData({ cost, talk });
      })
      .catch((err) => {
        console.error('Stats failed to load:', err);
        if (live) setFailed(true);
      });
    return () => {
      live = false;
    };
  }, [period]);

  const choose = (next: Period) => {
    setPeriod(next);
    saveStatsPeriod(next);
  };

  return (
    <div className="h-full overflow-y-auto bg-muted/50">
      <div className="mx-auto max-w-3xl space-y-8 p-8">
        <div className="flex items-center justify-between">
          <h1 className="text-3xl font-bold">Stats</h1>
          <div role="group" aria-label="Period" className="inline-flex rounded-lg border border-border bg-background p-1">
            {(['week', 'month'] as const).map((p) => (
              <button
                key={p}
                type="button"
                aria-pressed={period === p}
                onClick={() => choose(p)}
                className={`rounded-md px-3 py-1 text-sm ${period === p ? 'bg-muted font-medium' : 'text-muted-foreground hover:text-foreground'}`}
              >
                {p === 'week' ? 'Week' : 'Month'}
              </button>
            ))}
          </div>
        </div>

        {failed && <p role="alert" className="text-red-600">Could not load stats.</p>}
        {!failed && !data && <p className="text-muted-foreground">Loading…</p>}
        {data && (
          <>
            <CostSection periods={data.cost} period={period} />
            <TalkTimeSection key={period} period={period} talk={data.talk} />
          </>
        )}
      </div>
    </div>
  );
}
