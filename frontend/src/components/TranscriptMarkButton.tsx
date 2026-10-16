'use client';
import { useRef, useState } from 'react';
import { Popover, PopoverAnchor, PopoverContent } from '@/components/ui/popover';
import { MARKS, toggleMark } from '@/lib/meeting-notes';
import type { TranscriptMark } from '@/types';

interface Props {
  text: string;
  mark: TranscriptMark | null | undefined;
  onChange: (mark: TranscriptMark | null) => void;
}

export function TranscriptMarkButton({ text, mark, onChange }: Props) {
  const [open, setOpen] = useState(false);
  const anchorRef = useRef<HTMLDivElement>(null);
  const current = MARKS.find((m) => m.value === mark);

  const openUnlessSelecting = () => {
    if (window.getSelection()?.toString()) return; // the user is copying text
    setOpen(true);
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverAnchor asChild>
        <div
          ref={anchorRef}
          role="button"
          tabIndex={0}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-label={current ? `${current.label}: ${text}. Change mark` : `Mark line: ${text}`}
          onClick={openUnlessSelecting}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); setOpen(true); }
            else if (e.key === 'Escape') setOpen(false);
          }}
          className={`cursor-pointer select-text ${mark ? 'bg-amber-50 dark:bg-amber-950/30 rounded px-1 -mx-1' : ''}`}
        >
          <p className="text-base text-foreground leading-relaxed">
            {current && <span aria-hidden="true" className="mr-1">{current.icon}</span>}
            {text}
          </p>
        </div>
      </PopoverAnchor>
      {/* Radix would focus its (absent) trigger on close; send focus back to the line, unless the user moved it elsewhere */}
      <PopoverContent
        className="w-auto p-1"
        align="start"
        onCloseAutoFocus={(e) => {
          e.preventDefault();
          if (document.activeElement === document.body) anchorRef.current?.focus();
        }}
      >
        <div role="menu" className="flex gap-1">
          {MARKS.map((m) => (
            <button
              key={m.value}
              type="button"
              role="menuitemradio"
              aria-checked={mark === m.value}
              aria-label={`Mark as ${m.label}`}
              title={m.label}
              className={`rounded px-2 py-1 text-lg hover:bg-accent ${mark === m.value ? 'bg-accent' : ''}`}
              onClick={() => { onChange(toggleMark(mark, m.value)); setOpen(false); }}
            >
              {m.icon}
            </button>
          ))}
        </div>
      </PopoverContent>
    </Popover>
  );
}
