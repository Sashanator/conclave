import { describe, expect, test } from 'bun:test';
import { parseThemePreference, resolveTheme, THEME_INIT_SCRIPT, type ThemePreference } from '../../src/lib/theme';

/** Runs the inline <head> script against fakes and returns the theme it applied. */
function runInitScript(stored: string | null, systemDark: boolean) {
  const classes = new Set<string>();
  const style: Record<string, string> = {};
  const document = {
    documentElement: {
      classList: { toggle: (c: string, on: boolean) => (on ? classes.add(c) : classes.delete(c)) },
      style,
    },
  };
  const localStorage = { getItem: () => stored };
  const matchMedia = () => ({ matches: systemDark });
  new Function('document', 'localStorage', 'matchMedia', THEME_INIT_SCRIPT)(document, localStorage, matchMedia);
  return { dark: classes.has('dark'), colorScheme: style.colorScheme };
}

describe('theme', () => {
  test('parse falls back to system', () => {
    expect(parseThemePreference('light')).toBe('light');
    expect(parseThemePreference('dark')).toBe('dark');
    expect(parseThemePreference('system')).toBe('system');
    expect(parseThemePreference(null)).toBe('system');
    expect(parseThemePreference('purple')).toBe('system');
  });

  test('resolve: explicit choice wins, system follows Windows', () => {
    const cases: [ThemePreference, boolean, 'light' | 'dark'][] = [
      ['system', false, 'light'], ['system', true, 'dark'],
      ['light', true, 'light'], ['light', false, 'light'],
      ['dark', false, 'dark'], ['dark', true, 'dark'],
    ];
    for (const [pref, sys, want] of cases) expect(resolveTheme(pref, sys)).toBe(want);
  });

  test('the no-flash script applies exactly what resolveTheme says', () => {
    for (const stored of [null, 'system', 'light', 'dark', 'garbage']) {
      for (const systemDark of [false, true]) {
        const want = resolveTheme(parseThemePreference(stored), systemDark);
        const got = runInitScript(stored, systemDark);
        expect(got.dark).toBe(want === 'dark');
        expect(got.colorScheme).toBe(want);
      }
    }
  });

  test('the script never throws when storage is blocked', () => {
    const document = { documentElement: { classList: { toggle() {} }, style: {} } };
    const localStorage = { getItem: () => { throw new Error('blocked'); } };
    expect(() => new Function('document', 'localStorage', 'matchMedia', THEME_INIT_SCRIPT)(document, localStorage, () => ({ matches: false }))).not.toThrow();
  });
});
