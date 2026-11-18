'use client';

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { readSidebarOpen, writeSidebarOpen, type AskScope, type ScreenKind } from '@/lib/ai-sidebar';

type Ctx = {
  kind: ScreenKind | null;
  open: boolean;
  scope: AskScope | null;
  toggle(): void;
  close(): void;
  setScreen(kind: ScreenKind | null, scope: AskScope | null): void;
};

const AiSidebarContext = createContext<Ctx | null>(null);

export function AiSidebarProvider({ children }: { children: ReactNode }) {
  const [screen, setScreenState] = useState<{ kind: ScreenKind | null; scope: AskScope | null }>({ kind: null, scope: null });
  const [open, setOpen] = useState(false);
  const { kind } = screen;

  const setScreen = useCallback((k: ScreenKind | null, scope: AskScope | null) => {
    setScreenState({ kind: k, scope });
    setOpen(k ? readSidebarOpen(k) : false);
  }, []);

  const set = useCallback(
    (next: boolean) => {
      setOpen(next);
      if (kind) writeSidebarOpen(kind, next);
    },
    [kind],
  );
  const toggle = useCallback(() => set(!open), [set, open]);
  const close = useCallback(() => set(false), [set]);

  useEffect(() => {
    if (!kind) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey && (e.key.toLowerCase() === 'j' || e.code === 'KeyJ')) {
        e.preventDefault();
        toggle();
      } else if (e.key === 'Escape' && open && !e.defaultPrevented) {
        close();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [kind, open, toggle, close]);

  const value = useMemo(
    () => ({ kind, open, scope: screen.scope, toggle, close, setScreen }),
    [kind, open, screen.scope, toggle, close, setScreen],
  );
  return <AiSidebarContext.Provider value={value}>{children}</AiSidebarContext.Provider>;
}

export function useAiSidebar() {
  const ctx = useContext(AiSidebarContext);
  if (!ctx) throw new Error('useAiSidebar must be used inside AiSidebarProvider');
  const { setScreen: _s, ...rest } = ctx;
  return rest;
}

/** A screen declares its kind (and Ask scope) while mounted; null hides the sidebar. */
export function useAiSidebarScreen(kind: ScreenKind | null, scope?: AskScope): void {
  const ctx = useContext(AiSidebarContext);
  const setScreen = ctx?.setScreen;
  const scopeKey = scope ? JSON.stringify(scope) : '';
  useEffect(() => {
    setScreen?.(kind, scopeKey ? (JSON.parse(scopeKey) as AskScope) : null);
    return () => setScreen?.(null, null);
  }, [kind, scopeKey, setScreen]);
}
