import { describe, expect, test } from 'bun:test';
import {
  applyResolvedTheme, parseThemePreference, resolveTheme, THEME_INIT_SCRIPT, windowThemeFor,
  type ResolvedTheme, type ThemePreference,
} from '../../src/lib/theme';

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
  return { dark: classes.has('dark'), conclave: classes.has('theme-conclave'), colorScheme: style.colorScheme };
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
    const cases: [ThemePreference, boolean, ResolvedTheme][] = [
      ['system', false, 'light'], ['system', true, 'dark'],
      ['light', true, 'light'], ['light', false, 'light'],
      ['dark', false, 'dark'], ['dark', true, 'dark'],
      ['conclave', false, 'conclave'], ['conclave', true, 'conclave'],
    ];
    for (const [pref, sys, want] of cases) expect(resolveTheme(pref, sys)).toBe(want);
  });

  test('the no-flash script applies exactly what resolveTheme says', () => {
    for (const stored of [null, 'system', 'light', 'dark', 'conclave', 'garbage']) {
      for (const systemDark of [false, true]) {
        const want = resolveTheme(parseThemePreference(stored), systemDark);
        const got = runInitScript(stored, systemDark);
        expect(got.dark).toBe(want !== 'light');
        expect(got.conclave).toBe(want === 'conclave');
        expect(got.colorScheme).toBe(want === 'light' ? 'light' : 'dark');
      }
    }
  });

  test('the script never throws when storage is blocked', () => {
    const document = { documentElement: { classList: { toggle() {} }, style: {} } };
    const localStorage = { getItem: () => { throw new Error('blocked'); } };
    expect(() => new Function('document', 'localStorage', 'matchMedia', THEME_INIT_SCRIPT)(document, localStorage, () => ({ matches: false }))).not.toThrow();
  });

  test('conclave parses, resolves and applies as a dark theme with its own class', () => {
    expect(parseThemePreference('conclave')).toBe('conclave');
    expect(resolveTheme('conclave', false)).toBe('conclave');
    const el = document.createElement('html');
    applyResolvedTheme(el, 'conclave');
    expect(el.classList.contains('dark') && el.classList.contains('theme-conclave')).toBe(true);
    expect(el.style.colorScheme).toBe('dark');
    applyResolvedTheme(el, 'light');
    expect(el.classList.contains('dark') || el.classList.contains('theme-conclave')).toBe(false);
  });

  test('the window title bar is dark for conclave', () => {
    expect(windowThemeFor('dark')).toBe('dark');
    expect(windowThemeFor('conclave')).toBe('dark');
    expect(windowThemeFor('system')).toBeNull();
    expect(windowThemeFor('light')).toBe('light');
  });
});
