'use client';

import { formatTokens, type CostPeriod, type Period } from '@/lib/stats';
import { formatMoney } from '@/lib/dates';

// "16 Oct" for a week (its Monday), "October 2026" for a month
export function periodLabel(start: string, period: Period): string {
  const [y, m, d] = start.split('-').map(Number);
  const date = new Date(y, m - 1, d);
  return period === 'week'
    ? date.toLocaleDateString('en-GB', { day: 'numeric', month: 'short' })
    : date.toLocaleDateString('en-GB', { month: 'long', year: 'numeric' });
}

/** periods[0] is the current period; the rest are the previous ones, newest first. */
export function CostSection({ periods, period }: { periods: CostPeriod[]; period: Period }) {
  const [current] = periods;
  if (!current) return null;

  return (
    <section aria-labelledby="stats-cost" className="space-y-4">
      <h2 id="stats-cost" className="text-xl font-semibold">Cost</h2>
      <div className="rounded-lg border border-border bg-card p-4 space-y-3">
        <div className="flex items-baseline justify-between">
          <span className="text-muted-foreground">{`This ${period}, from ${periodLabel(current.start, period)}`}</span>
          <span className="text-2xl font-semibold">{formatMoney(current.total_usd)}</span>
        </div>
        <ul className="divide-y divide-border">
          {current.providers.map((p) => (
            <li key={p.provider} className="flex items-center justify-between py-2 text-sm">
              <span className="font-medium">{p.provider}</span>
              <span className="text-muted-foreground">{`Uncached input ${formatTokens(p.input_tokens)} · Output ${formatTokens(p.output_tokens)}`}</span>
              <span>{formatMoney(p.cost_usd)}</span>
              {p.unpriced_calls > 0 && <span className="text-amber-600">{`${p.unpriced_calls} unpriced`}</span>}
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
