export type ThemePreference = 'system' | 'light' | 'dark';
export type ResolvedTheme = 'light' | 'dark';

export const THEME_STORAGE_KEY = 'theme';

export function parseThemePreference(value: string | null): ThemePreference {
  return value === 'light' || value === 'dark' ? value : 'system';
}

export function resolveTheme(pref: ThemePreference, systemDark: boolean): ResolvedTheme {
  return pref === 'system' ? (systemDark ? 'dark' : 'light') : pref;
}

export function applyResolvedTheme(el: HTMLElement, resolved: ResolvedTheme) {
  el.classList.toggle('dark', resolved === 'dark');
  el.style.colorScheme = resolved;
}

/** Inline <head> script: applies the saved theme before first paint, so dark mode never flashes light.
 *  Must agree with parseThemePreference + resolveTheme (pinned by tests/lib/theme.test.ts). */
export const THEME_INIT_SCRIPT =
  "try{var p=localStorage.getItem('theme');" +
  "var d=p==='dark'||(p!=='light'&&matchMedia('(prefers-color-scheme: dark)').matches);" +
  "document.documentElement.classList.toggle('dark',d);" +
  "document.documentElement.style.colorScheme=d?'dark':'light'}catch(e){}";
