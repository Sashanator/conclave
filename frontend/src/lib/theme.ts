export type ThemePreference = 'system' | 'light' | 'dark' | 'conclave';
export type ResolvedTheme = 'light' | 'dark' | 'conclave';

export const THEME_STORAGE_KEY = 'theme';

export function parseThemePreference(value: string | null): ThemePreference {
  return value === 'light' || value === 'dark' || value === 'conclave' ? value : 'system';
}

export function resolveTheme(pref: ThemePreference, systemDark: boolean): ResolvedTheme {
  return pref === 'system' ? (systemDark ? 'dark' : 'light') : pref;
}

export function applyResolvedTheme(el: HTMLElement, resolved: ResolvedTheme) {
  const dark = resolved !== 'light';
  el.classList.toggle('dark', dark);
  el.classList.toggle('theme-conclave', resolved === 'conclave');
  el.style.colorScheme = dark ? 'dark' : 'light';
}

/** Native title bar theme; null follows Windows. */
export function windowThemeFor(pref: ThemePreference): 'light' | 'dark' | null {
  return pref === 'system' ? null : pref === 'light' ? 'light' : 'dark';
}

/** Inline <head> script: applies the saved theme before first paint, so dark mode never flashes light.
 *  Must agree with parseThemePreference + resolveTheme (pinned by tests/lib/theme.test.ts). */
export const THEME_INIT_SCRIPT =
  "try{var p=localStorage.getItem('theme');var c=p==='conclave';" +
  "var d=c||p==='dark'||(p!=='light'&&matchMedia('(prefers-color-scheme: dark)').matches);" +
  "var e=document.documentElement;e.classList.toggle('dark',d);e.classList.toggle('theme-conclave',c);" +
  "e.style.colorScheme=d?'dark':'light'}catch(e){}";
