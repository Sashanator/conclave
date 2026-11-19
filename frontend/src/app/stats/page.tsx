'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { PageFrame } from '@/components/PageFrame/PageFrame';
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

  const toggle = (
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
  );

  return (
    <PageFrame variant="dashboard" title="Stats" actions={toggle}>
      <div className="space-y-8">
        {failed && <p role="alert" className="text-red-600">Could not load stats.</p>}
        {!failed && !data && <p className="text-muted-foreground">Loading…</p>}
        {data && (
          <>
            <div className="grid gap-6 2xl:grid-cols-[minmax(0,5fr)_minmax(0,7fr)]">
              <CostSection periods={data.cost} period={period} />
            </div>
            <TalkTimeSection key={period} period={period} talk={data.talk} />
          </>
        )}
      </div>
    </PageFrame>
  );
}
