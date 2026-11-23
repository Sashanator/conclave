'use client';

import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { Square, X } from 'lucide-react';
import { hasText, liveLabel, mergeView, pillTone, showsControls, PillView } from '@/lib/dictatePill';

const DOT: Record<ReturnType<typeof pillTone>, string> = {
  live: 'bg-red-500 animate-pulse motion-reduce:animate-none',
  busy: 'bg-sky-300 animate-pulse motion-reduce:animate-none',
  ok: 'bg-emerald-400',
  warn: 'bg-amber-400',
};

// Committed text in the foreground colour, volatile text muted (no aria-live: the text changes too often to announce).
const liveText = (v: PillView) => (
  <>
    <span className="text-foreground">{v.committed}</span>
    {v.volatile && <span className="text-muted-foreground">{v.committed ? ' ' : ''}{v.volatile}</span>}
  </>
);

const BAR_SCALE = [0.5, 0.8, 1, 0.8, 0.5];

export default function DictatePill() {
  const [view, setView] = useState<PillView | null>(null);
  const [clipped, setClipped] = useState(false);
  // The palette class is read once; the pill window lives for one dictation.
  const [conclave, setConclave] = useState(false);
  useEffect(() => {
    setConclave(document.documentElement.classList.contains('theme-conclave'));
  }, []);
  const cardRef = useRef<HTMLDivElement>(null);
  const textRef = useRef<HTMLDivElement>(null);
  const paraRef = useRef<HTMLParagraphElement>(null);
  const sentSize = useRef('360x56');

  useEffect(() => {
    let alive = true;
    let unlisten: UnlistenFn | undefined;
    listen<PillView>('dictate-pill', (e) => setView((prev) => mergeView(prev, e.payload)))
      .then((stop) => {
        if (!alive) {
          stop();
          return;
        }
        unlisten = stop;
        // Events sent before the listener existed are lost, so read the current view once it is registered.
        invoke<PillView | null>('dictate_pill_view')
          .then((v) => {
            if (alive && v) setView((prev) => mergeView(prev, v));
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

  const text = !!view && hasText(view);
  const panel = text && view.style === 'panel';
  const line = view?.style === 'line'; // the card fills its 560 window with or without text
  const body = `${view?.committed ?? ''}${view?.volatile ?? ''}`;

  // The Panel window opens at 360x56: grow it to the measured card while text shows, shrink back when it is gone.
  useLayoutEffect(() => {
    if (!view || view.style === 'line') return;
    // Under justify-end the overflow goes above the box, so scrollHeight never shows it: compare the text with the box.
    if (panel && textRef.current && paraRef.current) setClipped(paraRef.current.offsetHeight > textRef.current.clientHeight - 12);
    const size = panel && cardRef.current ? { width: 480, height: cardRef.current.offsetHeight + 16 } : { width: 360, height: 56 };
    const key = `${size.width}x${size.height}`;
    if (key === sentSize.current) return;
    sentSize.current = key;
    invoke('dictate_pill_resize', size).catch(() => {});
  }, [view?.style, panel, body]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!view) return null;
  const showsLevel = view.state === 'listening' || view.state === 'hands-free';
  const level = Math.min(1, Math.max(0, view.level));

  return (
    <main className={`flex h-screen w-screen select-none justify-center ${panel ? 'items-end pb-2' : 'items-center'}`}>
      <h1 className="sr-only">Dictation</h1>
      {/* `dark` forces the dark palette on this element, so the pill is dark in both themes. */}
      <div
        ref={cardRef}
        className={`dark${conclave ? ' theme-conclave' : ''} flex shrink-0 items-center bg-background text-foreground shadow-lg ${
          panel
            ? 'w-[464px] flex-col rounded-2xl text-sm font-medium'
            : line
              ? 'h-10 w-[544px] gap-2.5 rounded-full px-4 text-sm font-medium'
              : 'h-10 max-w-[344px] gap-2.5 rounded-full px-4 text-sm font-medium'
        }`}
      >
        {panel && (
          <div
            ref={textRef}
            className={`flex max-h-[78px] w-full flex-col justify-end overflow-hidden px-4 pt-3 text-left text-[15px] font-normal leading-[22px] ${
              clipped ? '[mask-image:linear-gradient(to_bottom,transparent,black_22px)]' : ''
            }`}
          >
            <p ref={paraRef}>{liveText(view)}</p>
          </div>
        )}
        <div className={panel ? 'flex h-10 w-full items-center gap-2.5 px-4' : 'contents'}>
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
        {line && (
          <span className="flex min-w-0 flex-1 justify-end overflow-hidden whitespace-nowrap font-normal [mask-image:linear-gradient(to_right,transparent,black_32px)]">
            <span>{liveText(view)}</span>
          </span>
        )}
        <span className="min-w-0 truncate tabular-nums">{liveLabel(view)}</span>
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
    </main>
  );
}
