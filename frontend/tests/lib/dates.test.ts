import { describe, expect, test } from 'bun:test';
import { formatDayLabel, formatLongDate, formatMoney, formatRowDate } from '../../src/lib/dates';

const d = (y: number, m: number, day: number, h = 0, min = 0) => new Date(y, m, day, h, min);
const now = d(2026, 10, 18, 12);

describe('dates', () => {
  test('formatRowDate', () => {
    expect(formatRowDate(d(2026, 10, 18, 7, 56), d(2026, 10, 18, 8, 32), now)).toBe('Wed 18 Nov · 07:56–08:32');
    expect(formatRowDate(d(2026, 10, 18, 7, 56), undefined, now)).toBe('Wed 18 Nov · 07:56');
    expect(formatRowDate(d(2026, 10, 18, 7, 56), null, now)).toBe('Wed 18 Nov · 07:56');
    expect(formatRowDate(d(2025, 10, 14, 8, 0), d(2025, 10, 14, 8, 50), d(2026, 10, 18))).toBe('Fri 14 Nov 2025 · 08:00–08:50');
  });
  test('formatLongDate', () => {
    expect(formatLongDate(d(2026, 10, 18))).toBe('Wednesday 18 Nov 2026');
  });
  test('formatDayLabel', () => {
    expect(formatDayLabel(d(2026, 10, 18, 9), now)).toBe('Today · Wednesday 18 Nov');
    expect(formatDayLabel(d(2026, 10, 17), now)).toBe('Tuesday 17 Nov');
    expect(formatDayLabel(d(2025, 10, 14), now)).toBe('Friday 14 Nov 2025');
  });
  test('formatMoney', () => {
    expect(formatMoney(1.44)).toBe('$1.44');
    expect(formatMoney(0)).toBe('$0.00');
    expect(formatMoney(1234.5)).toBe('$1234.50');
    expect(formatMoney(0.004)).toBe('<$0.01');
  });
  test('no numeric slash dates', () => {
    const all = [formatRowDate(d(2026, 10, 18, 7), null, now), formatLongDate(now), formatDayLabel(now, now)];
    for (const s of all) expect(s).not.toMatch(/\d+\/\d+/);
  });
});
