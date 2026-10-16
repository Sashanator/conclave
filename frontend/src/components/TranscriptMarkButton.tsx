'use client';
import { useState } from 'react';
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
  const icon = MARKS.find((m) => m.value === mark)?.icon;

  const openUnlessSelecting = () => {
    if (window.getSelection()?.toString()) return; // the user is copying text
    setOpen(true);
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverAnchor asChild>
        <div
          role="button"
          tabIndex={0}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-label={`Mark line: ${text}`}
          onClick={openUnlessSelecting}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); setOpen(true); }
            else if (e.key === 'Escape') setOpen(false);
          }}
          className={`cursor-pointer select-text ${mark ? 'bg-amber-50 dark:bg-amber-950/30 rounded px-1 -mx-1' : ''}`}
        >
          <p className="text-base text-foreground leading-relaxed">
            {icon && <span aria-hidden="true" className="mr-1">{icon}</span>}
            {text}
          </p>
        </div>
      </PopoverAnchor>
      <PopoverContent className="w-auto p-1" align="start">
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
