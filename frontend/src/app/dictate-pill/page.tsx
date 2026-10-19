'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { Square, X } from 'lucide-react';
import { pillLabel, pillTone, showsControls, PillView } from '@/lib/dictatePill';

const DOT: Record<ReturnType<typeof pillTone>, string> = {
  live: 'bg-red-500 animate-pulse',
  busy: 'bg-sky-300 animate-pulse',
  ok: 'bg-emerald-400',
  warn: 'bg-amber-400',
};

const BAR_SCALE = [0.5, 0.8, 1, 0.8, 0.5];

export default function DictatePill() {
  const [view, setView] = useState<PillView | null>(null);

  useEffect(() => {
    let alive = true;
    let unlisten: UnlistenFn | undefined;
    listen<PillView>('dictate-pill', (e) => setView(e.payload))
      .then((stop) => {
        if (!alive) {
          stop();
          return;
        }
        unlisten = stop;
        // Events sent before the listener existed are lost, so read the current view once it is registered.
        invoke<PillView | null>('dictate_pill_view')
          .then((v) => {
            if (alive && v) setView(v);
          })
          .catch(() => {});
      })
      .catch(() => {});
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  // Same rule as the banner and AppShell: no context menu in production builds.
  useEffect(() => {
    if (process.env.NODE_ENV === 'production') {
      const handleContextMenu = (e: MouseEvent) => e.preventDefault();
      document.addEventListener('contextmenu', handleContextMenu);
      return () => document.removeEventListener('contextmenu', handleContextMenu);
    }
  }, []);

  const act = (action: 'stop' | 'cancel') => invoke('dictate_pill_action', { action }).catch(() => {});

  if (!view) return null;
  const showsLevel = view.state === 'listening' || view.state === 'hands-free';
  const level = Math.min(1, Math.max(0, view.level));

  return (
    <div className="flex h-screen w-screen select-none items-center justify-center">
      {/* `dark` forces the dark palette on this element, so the pill is dark in both themes. */}
      <div className="dark flex h-10 max-w-[344px] items-center gap-2.5 rounded-full bg-background px-4 text-sm font-medium text-foreground shadow-lg">
        <span className={`h-2.5 w-2.5 shrink-0 rounded-full ${DOT[pillTone(view.state)]}`} aria-hidden />
        {showsLevel && (
          <span className="flex h-5 shrink-0 items-center gap-0.5" aria-hidden>
            {BAR_SCALE.map((scale, i) => (
              <span
                key={i}
                className="w-1 rounded-full bg-foreground/80"
                style={{ height: `${4 + 16 * level * scale}px` }}
              />
            ))}
          </span>
        )}
        <span className="min-w-0 truncate tabular-nums">{pillLabel(view)}</span>
        {showsControls(view.state) && (
          <>
            <button
              onClick={() => act('stop')}
              aria-label="Stop"
              className="shrink-0 rounded-full p-1 hover:opacity-70"
            >
              <Square className="h-3 w-3 fill-current" />
            </button>
            <button
              onClick={() => act('cancel')}
              aria-label="Cancel"
              className="shrink-0 rounded-full p-1 hover:opacity-70"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </>
        )}
      </div>
    </div>
  );
}
