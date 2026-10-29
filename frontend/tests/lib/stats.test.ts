import { describe, expect, test } from 'bun:test';
import { formatTokens, formatUsd, purposeLabel, readStatsPeriod, saveStatsPeriod, sharePct } from '../../src/lib/stats';

describe('formatUsd', () => {
  test('always three decimals, dash for unpriced', () => {
    expect(formatUsd(0.004)).toBe('$0.004');
    expect(formatUsd(0.012)).toBe('$0.012');
    expect(formatUsd(1.27)).toBe('$1.270');
    expect(formatUsd(0)).toBe('$0.000');
    expect(formatUsd(null)).toBe('—');
  });
});

describe('formatTokens', () => {
  test('plain, thousands and millions', () => {
    expect(formatTokens(950)).toBe('950');
    expect(formatTokens(18400)).toBe('18.4k');
    expect(formatTokens(1_250_000)).toBe('1.25M');
  });

  test('rounding into the next unit reads as millions', () => {
    expect(formatTokens(999_950)).toBe('1.00M');
  });
});

describe('sharePct', () => {
  test('rounds to a whole percent', () => {
    expect(sharePct(38, 100)).toBe('38%');
    expect(sharePct(1, 3)).toBe('33%');
  });

  test('no talk time gives a dash', () => {
    expect(sharePct(0, 0)).toBe('—');
  });
});

describe('stats period storage', () => {
  test('defaults to week and round-trips month', () => {
    expect(readStatsPeriod()).toBe('week');
    saveStatsPeriod('month');
    expect(readStatsPeriod()).toBe('month');
    saveStatsPeriod('week');
    expect(readStatsPeriod()).toBe('week');
  });

  test('a storage that throws falls back to week', () => {
    const storage = globalThis.localStorage;
    Object.defineProperty(globalThis, 'localStorage', {
      configurable: true,
      get() {
        throw new Error('storage disabled');
      },
    });
    try {
      expect(readStatsPeriod()).toBe('week');
      expect(() => saveStatsPeriod('month')).not.toThrow();
    } finally {
      Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: storage });
    }
  });
});

describe('purposeLabel', () => {
  test('plain labels for the recorded purpose ids, raw id for unknown ones', () => {
    expect(purposeLabel('template_pick')).toBe('Template choice');
    expect(purposeLabel('connection_test')).toBe('Connection test');
    expect(purposeLabel('extraction')).toBe('Task extraction');
    expect(purposeLabel('prep')).toBe('Meeting prep');
    expect(purposeLabel('something_new')).toBe('something_new');
    expect(purposeLabel('constructor')).toBe('constructor');
  });
});
