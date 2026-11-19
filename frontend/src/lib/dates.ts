import { format, isSameDay } from 'date-fns';

const yearSuffix = (d: Date, now: Date) => (d.getFullYear() === now.getFullYear() ? '' : ' yyyy');

/** `Wed 18 Nov · 07:56–08:32`; the year only when it is not the current one. */
export function formatRowDate(start: Date, end?: Date | null, now: Date = new Date()): string {
  const range = end ? `${format(start, 'HH:mm')}–${format(end, 'HH:mm')}` : format(start, 'HH:mm');
  return `${format(start, `EEE d MMM${yearSuffix(start, now)}`)} · ${range}`;
}

export const formatLongDate = (d: Date): string => format(d, 'EEEE d MMM yyyy');

export function formatDayLabel(d: Date, now: Date): string {
  const label = format(d, `EEEE d MMM${yearSuffix(d, now)}`);
  return isSameDay(d, now) ? `Today · ${label}` : label;
}

/** A fraction of a cent must not read as free. */
export const formatMoney = (usd: number): string => (usd > 0 && usd < 0.005 ? '<$0.01' : `$${usd.toFixed(2)}`);
