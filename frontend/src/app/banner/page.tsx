'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Mic, X } from 'lucide-react';

interface BannerView {
  kind: 'detected' | 'ended' | 'mention';
  app: string;
  certain: boolean;
  line: string | null;
}

export default function MeetingBanner() {
  const [view, setView] = useState<BannerView | null>(null);

  useEffect(() => {
    invoke<BannerView | null>('get_meeting_banner').then(setView).catch(() => setView(null));
  }, []);

  // Same rule as AppShell in layout.tsx: no context menu in production builds.
  useEffect(() => {
    if (process.env.NODE_ENV === 'production') {
      const handleContextMenu = (e: MouseEvent) => e.preventDefault();
      document.addEventListener('contextmenu', handleContextMenu);
      return () => document.removeEventListener('contextmenu', handleContextMenu);
    }
  }, []);

  const act = (action: 'record' | 'open' | 'dismiss') => invoke('meeting_banner_action', { action }).catch(() => {});

  if (!view) return null;
  if (view.kind === 'mention') {
    return (
      <div className="flex h-screen w-screen select-none items-center gap-2 overflow-hidden rounded-xl border border-border bg-card pr-2 text-card-foreground">
        <button
          onClick={() => act('open')}
          className="flex min-w-0 flex-1 flex-col items-start gap-0.5 px-4 text-left"
        >
          <span className="text-sm font-medium">Someone said your name</span>
          <span className="w-full truncate text-xs text-muted-foreground">{view.line}</span>
        </button>
        <button
          onClick={() => act('dismiss')}
          aria-label="Dismiss"
          className="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-accent-foreground"
        >
          <X className="h-4 w-4" />
        </button>
      </div>
    );
  }
  const text =
    view.kind === 'ended'
      ? `${view.app} meeting ended. Notes are being generated.`
      : view.certain
        ? `${view.app} meeting detected`
        : 'Browser is using the microphone';

  return (
    <div className="flex h-screen w-screen select-none items-center gap-3 overflow-hidden rounded-xl border border-border bg-card px-4 text-card-foreground">
      <Mic className="h-5 w-5 shrink-0 text-primary" aria-hidden />
      <span className="flex-1 truncate text-sm font-medium">{text}</span>
      {view.kind === 'detected' && (
        <button
          onClick={() => act('record')}
          className="rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90"
        >
          Record
        </button>
      )}
      <button
        onClick={() => act('dismiss')}
        aria-label="Dismiss"
        className="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-accent-foreground"
      >
        {view.kind === 'detected' ? <span className="text-sm">Dismiss</span> : <X className="h-4 w-4" />}
      </button>
    </div>
  );
}
