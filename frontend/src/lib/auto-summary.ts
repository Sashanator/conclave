import { parseDbTime } from '@/lib/meetings';

const DAY_MS = 24 * 60 * 60 * 1000;

/**
 * Whether opening a meeting page may start its summary on its own.
 * `recording`: the page was opened by finishing a recording. Otherwise (a restart or an update
 * cut the first attempt short) the meeting must have ended within the last 24 h; an old or
 * still-running meeting never summarizes just because it was opened.
 */
export function autoSummaryAllowed(o: { source: string | null; autoSummaryOn: boolean; endedAt: string | null | undefined; now: Date }): boolean {
  if (!o.autoSummaryOn) return false;
  if (o.source === 'recording') return true;
  if (!o.endedAt) return false;
  const age = o.now.getTime() - parseDbTime(o.endedAt).getTime();
  return age >= 0 && age <= DAY_MS; // NaN (bad date) fails both
}

// Meetings whose automatic attempt already ran this app session. Some failures happen before the
// backend writes a summary row, so "idle" alone would retry (and toast again) on every reopen.
const attempted = new Set<string>();
export const markAutoAttempt = (id: string) => void attempted.add(id);
export const autoAttempted = (id: string) => attempted.has(id);
