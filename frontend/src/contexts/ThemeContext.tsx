'use client';

import { createContext, useCallback, useContext, useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  applyResolvedTheme, parseThemePreference, resolveTheme, THEME_STORAGE_KEY, windowThemeFor,
  type ResolvedTheme, type ThemePreference,
} from '@/lib/theme';

interface ThemeContextValue {
  preference: ThemePreference;
  resolved: ResolvedTheme;
  setPreference: (pref: ThemePreference) => void;
}

const ThemeContext = createContext<ThemeContextValue>({
  preference: 'system',
  resolved: 'light',
  setPreference: () => {},
});

function readStored(): ThemePreference {
  try {
    return parseThemePreference(localStorage.getItem(THEME_STORAGE_KEY));
  } catch {
    return 'system';
  }
}

const systemQuery = () => window.matchMedia('(prefers-color-scheme: dark)');

// The native title bar follows the choice; null = follow Windows. Outside Tauri (a plain browser, tests)
// there is no window, and getCurrentWindow() throws synchronously, so skip it there.
function syncWindowTheme(pref: ThemePreference) {
  if (!('__TAURI_INTERNALS__' in window)) return;
  try {
    getCurrentWindow().setTheme(windowThemeFor(pref)).catch(() => {});
  } catch {
    // Tauri present but the window API unavailable: the in-page theme still applies.
  }
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  // Lazy initialisers read the same values the <head> script used, so the first client render matches the page.
  const [preference, setPref] = useState<ThemePreference>(() => (typeof window === 'undefined' ? 'system' : readStored()));
  const [systemDark, setSystemDark] = useState(() => typeof window !== 'undefined' && systemQuery().matches);
  const resolved = resolveTheme(preference, systemDark);

  useEffect(() => {
    const mq = systemQuery();
    const onChange = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    mq.addEventListener('change', onChange);
    // Other Conclave windows (the meeting banner) share localStorage and pick up a change here.
    const onStorage = (e: StorageEvent) => {
      if (e.key === THEME_STORAGE_KEY) setPref(parseThemePreference(e.newValue));
    };
    window.addEventListener('storage', onStorage);
    return () => {
      mq.removeEventListener('change', onChange);
      window.removeEventListener('storage', onStorage);
    };
  }, []);

  useEffect(() => {
    applyResolvedTheme(document.documentElement, resolved);
  }, [resolved]);

  useEffect(() => {
    syncWindowTheme(preference);
  }, [preference]);

  const setPreference = useCallback((pref: ThemePreference) => {
    try {
      localStorage.setItem(THEME_STORAGE_KEY, pref);
    } catch {
      // Storage blocked: the choice still applies for this session.
    }
    setPref(pref);
  }, []);

  return <ThemeContext.Provider value={{ preference, resolved, setPreference }}>{children}</ThemeContext.Provider>;
}

export const useTheme = () => useContext(ThemeContext);
