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
  /** Fixed px for the handle and gaps that sit between the two panes, so the handle centres on the pointer (ratio mode). */
  gutter?: number;
  /** Px mode: `ratio` is a width in px (min/max/default in px). `end` measures from the container's right edge. */
  px?: 'start' | 'end';
}

export function useSplitRatio({ storageKey, defaultRatio, min, max, offset = 0, label, gutter = 0, px }: SplitOptions) {
  // Mutable so two splitters can share one container (Task 10).
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [ratio, setRatio] = useState(defaultRatio);
  const dragging = useRef(false);
  const [isDragging, setIsDragging] = useState(false);
  const step = px ? 16 : 0.05;
  const clamp = useCallback((v: number) => Math.min(max, Math.max(min, v)), [min, max]);

  useEffect(() => { setRatio(readStoredRatio(storageKey, defaultRatio, min, max)); }, [storageKey, defaultRatio, min, max]);

  const onPointerDown = useCallback((e: React.PointerEvent) => {
    e.preventDefault();
    dragging.current = true;
    setIsDragging(true);
    e.currentTarget.setPointerCapture(e.pointerId);
  }, []);
  const onPointerMove = useCallback((e: React.PointerEvent) => {
    if (!dragging.current || !containerRef.current) return;
    const rect = containerRef.current.getBoundingClientRect();
    if (px) setRatio(clamp(Math.round(px === 'end' ? rect.right - e.clientX - offset : e.clientX - rect.left - offset)));
    else setRatio(clamp((e.clientX - rect.left - gutter / 2) / (rect.width - gutter) - offset));
  }, [clamp, offset, gutter, px]);
  const onPointerUp = useCallback(() => {
    if (!dragging.current) return;
    dragging.current = false;
    setIsDragging(false);
    setRatio((current) => { writeStoredRatio(storageKey, current); return current; });
  }, [storageKey]);
  const onKeyDown = useCallback((e: React.KeyboardEvent) => {
    const grow = px === 'end' ? 'ArrowLeft' : 'ArrowRight';
    const shrink = px === 'end' ? 'ArrowRight' : 'ArrowLeft';
    const next =
      e.key === shrink ? clamp(ratio - step) :
      e.key === grow ? clamp(ratio + step) :
      e.key === 'Home' ? min :
      e.key === 'End' ? max : null;
    if (next === null) return;
    e.preventDefault();
    setRatio(next);
    writeStoredRatio(storageKey, next);
  }, [ratio, clamp, min, max, storageKey, px, step]);

  const pct = (v: number) => (px ? Math.round(v) : Math.round(v * 100));
  return {
    ratio,
    dragging: isDragging,
    containerRef,
    containerHandlers: { onPointerMove, onPointerUp, onPointerCancel: onPointerUp },
    separatorProps: {
      role: 'separator' as const,
      'aria-orientation': 'vertical' as const,
      'aria-valuenow': pct(ratio),
      'aria-valuemin': pct(min),
      'aria-valuemax': pct(max),
      'aria-valuetext': `${label} ${pct(ratio)} ${px ? 'pixels' : 'percent'}`,
      'aria-label': `Resize ${label}`,
      tabIndex: 0,
      onPointerDown,
      onKeyDown,
    },
  };
}

export function SplitHandle({ className = 'hidden md:flex', ...props }: Record<string, unknown> & { className?: string }) {
  return (
    <div
      {...props}
      className={`group relative z-10 hidden w-2 flex-shrink-0 cursor-col-resize items-stretch justify-center focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-inset ${className}`}
    >
      <div className="h-full w-px bg-border transition-[width,background-color] duration-150 ease-out group-hover:w-1 group-hover:bg-blue-400 group-active:w-1 group-active:bg-blue-500" />
    </div>
  );
}
