'use client';

import { formatMoney } from '@/lib/dates';
import type { CostPeriod, Period } from '@/lib/stats';
import { periodLabel } from './CostSection';

const round1 = (n: number) => Math.round(n * 10) / 10;

// A month name that repeats among the bars (13 months) gets its year
function barLabel(start: string, period: Period, withYear: boolean): string {
  if (period === 'week') return periodLabel(start, period);
  const [y, m, d] = start.split('-').map(Number);
  return new Date(y, m - 1, d).toLocaleDateString('en-GB', withYear ? { month: 'short', year: 'numeric' } : { month: 'short' });
}

/** periods[0] is the current period; bars run oldest to newest, so the current one is last. */
export function CostChart({ periods, period }: { periods: CostPeriod[]; period: Period }) {
  if (periods.length === 0) return null;
  const title = period === 'week' ? 'Weekly cost' : 'Monthly cost';
  const bars = [...periods].reverse();
  const monthOf = (start: string) => Number(start.split('-')[1]);
  const repeats = (start: string) => bars.filter((b) => monthOf(b.start) === monthOf(start)).length > 1;
  const max = Math.max(...bars.map((p) => p.total_usd));

  return (
    <section aria-labelledby="stats-chart" className="space-y-4">
      <div>
        <h2 id="stats-chart" className="text-xl font-semibold">{title}</h2>
        <p className="text-sm text-muted-foreground">
          {period === 'week' ? 'Last 8 weeks and this week' : 'Last 12 months and this month'}
        </p>
      </div>
      <ol aria-label={title} className="flex h-44 items-end gap-3">
        {bars.map((p, i) => {
          const pct = max > 0 ? Math.max(1, round1((p.total_usd / max) * 100)) : 1;
          const current = i === bars.length - 1;
          return (
            <li
              key={p.start}
              aria-current={current ? 'true' : undefined}
              className={`flex h-full flex-1 flex-col items-center justify-end text-xs ${current ? 'text-foreground font-medium' : 'text-muted-foreground'}`}
            >
              {formatMoney(p.total_usd)}
              <span className="block w-full max-w-14 rounded-t bg-primary" style={{ height: `${pct}%` }} />
              {barLabel(p.start, period, repeats(p.start))}
            </li>
          );
        })}
      </ol>
    </section>
  );
}
