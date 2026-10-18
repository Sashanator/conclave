'use client';
import { useRef, useState } from 'react';
import { Popover, PopoverAnchor, PopoverContent } from '@/components/ui/popover';
import { lineSpeakerName, type SpeakerView } from '@/lib/speakers';

interface Props {
  idx: number | null | undefined;
  speakers: SpeakerView[];
  /** While a job runs the label is plain text */
  disabled: boolean;
  onMove: (idx: number | null) => void;
}

// The line's speaker label; clicking it moves the line to another speaker of the meeting or to Unknown
export function SpeakerLineMenu({ idx, speakers, disabled, onMove }: Props) {
  const [open, setOpen] = useState(false);
  const anchorRef = useRef<HTMLSpanElement>(null);
  const name = lineSpeakerName(idx, speakers, true);
  const labelClass = 'text-xs text-muted-foreground mt-1 flex-shrink-0';

  if (disabled) {
    return <span data-speaker-label={name} className={labelClass}>{name}</span>;
  }

  const choose = (next: number | null) => {
    setOpen(false);
    if (next !== (idx ?? null)) onMove(next);
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverAnchor asChild>
        <span
          ref={anchorRef}
          role="button"
          tabIndex={0}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-label={`Speaker: ${name}. Change speaker`}
          data-speaker-label={name}
          onClick={() => setOpen(true)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); setOpen(true); }
            else if (e.key === 'Escape') setOpen(false);
          }}
          className={`${labelClass} cursor-pointer hover:underline`}
        >
          {name}
        </span>
      </PopoverAnchor>
      {/* Same focus return as TranscriptMarkButton: back to the label unless the user moved elsewhere */}
      <PopoverContent
        className="w-auto p-1"
        align="start"
        onCloseAutoFocus={(e) => {
          e.preventDefault();
          if (document.activeElement === document.body) anchorRef.current?.focus();
        }}
      >
        <div role="menu" className="flex flex-col">
          {speakers.map((s) => (
            <button
              key={s.idx}
              type="button"
              role="menuitemradio"
              aria-checked={idx === s.idx}
              className={`rounded px-2 py-1 text-left text-sm hover:bg-accent ${idx === s.idx ? 'bg-accent' : ''}`}
              onClick={() => choose(s.idx)}
            >
              {s.name}
            </button>
          ))}
          <button
            type="button"
            role="menuitemradio"
            aria-checked={idx == null}
            className={`rounded px-2 py-1 text-left text-sm hover:bg-accent ${idx == null ? 'bg-accent' : ''}`}
            onClick={() => choose(null)}
          >
            Unknown
          </button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
