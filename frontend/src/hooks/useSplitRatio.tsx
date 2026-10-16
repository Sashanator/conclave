'use client';
import { useCallback, useEffect, useRef, useState } from 'react';
import { readStoredRatio, writeStoredRatio } from '@/lib/meeting-page-layout';

interface SplitOptions {
  storageKey: string;
  defaultRatio: number;
  min: number;
  max: number;
  /** Subtracted from the pointer position, for a second splitter in the same container. */
  offset?: number;
  label: string;
}

export function useSplitRatio({ storageKey, defaultRatio, min, max, offset = 0, label }: SplitOptions) {
  // Mutable so two splitters can share one container (Task 10).
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [ratio, setRatio] = useState(defaultRatio);
  const dragging = useRef(false);
  const clamp = useCallback((v: number) => Math.min(max, Math.max(min, v)), [min, max]);

  useEffect(() => { setRatio(readStoredRatio(storageKey, defaultRatio, min, max)); }, [storageKey, defaultRatio, min, max]);

  const onPointerDown = useCallback((e: React.PointerEvent) => {
    e.preventDefault();
    dragging.current = true;
    e.currentTarget.setPointerCapture(e.pointerId);
  }, []);
  const onPointerMove = useCallback((e: React.PointerEvent) => {
    if (!dragging.current || !containerRef.current) return;
    const rect = containerRef.current.getBoundingClientRect();
    setRatio(clamp((e.clientX - rect.left) / rect.width - offset));
  }, [clamp, offset]);
  const onPointerUp = useCallback(() => {
    if (!dragging.current) return;
    dragging.current = false;
    setRatio((current) => { writeStoredRatio(storageKey, current); return current; });
  }, [storageKey]);
  const onKeyDown = useCallback((e: React.KeyboardEvent) => {
    const next =
      e.key === 'ArrowLeft' ? clamp(ratio - 0.05) :
      e.key === 'ArrowRight' ? clamp(ratio + 0.05) :
      e.key === 'Home' ? min :
      e.key === 'End' ? max : null;
    if (next === null) return;
    e.preventDefault();
    setRatio(next);
    writeStoredRatio(storageKey, next);
  }, [ratio, clamp, min, max, storageKey]);

  return {
    ratio,
    containerRef,
    containerHandlers: { onPointerMove, onPointerUp, onPointerCancel: onPointerUp },
    separatorProps: {
      role: 'separator' as const,
      'aria-orientation': 'vertical' as const,
      'aria-valuenow': Math.round(ratio * 100),
      'aria-valuemin': Math.round(min * 100),
      'aria-valuemax': Math.round(max * 100),
      'aria-valuetext': `${label} ${Math.round(ratio * 100)} percent`,
      'aria-label': `Resize ${label}`,
      tabIndex: 0,
      onPointerDown,
      onKeyDown,
    },
  };
}

export function SplitHandle(props: Record<string, unknown>) {
  return (
    <div
      {...props}
      className="group relative z-10 hidden w-2 flex-shrink-0 cursor-col-resize items-stretch justify-center focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-inset md:flex"
    >
      <div className="h-full w-px bg-border transition-[width,background-color] duration-150 ease-out group-hover:w-1 group-hover:bg-blue-400 group-active:w-1 group-active:bg-blue-500" />
    </div>
  );
}
