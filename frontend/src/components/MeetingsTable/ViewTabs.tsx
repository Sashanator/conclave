'use client';

import { useRef, useState } from 'react';
import { MoreHorizontal } from 'lucide-react';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import type { MeetingView } from '@/lib/meetings';

const tab = 'border-b-2 -mb-px px-2.5 py-1.5 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring';

export function ViewTabs({ views, activeId, counts, onSelect, onAdd, onRename, onDelete }: {
  views: MeetingView[];
  activeId: string | null;
  counts: Record<string, number>;
  onSelect: (id: string) => void;
  onAdd: () => void;
  onRename: (id: string, name: string) => void;
  onDelete: (id: string) => void;
}) {
  const [renaming, setRenaming] = useState<string | null>(null);
  const done = useRef(false); // Enter and the blur it causes must commit once
  const finish = (id: string, value: string, save: boolean) => {
    if (done.current) return;
    done.current = true;
    setRenaming(null);
    const name = value.trim();
    if (save && name) onRename(id, name);
  };

  return (
    <div className="mb-2.5 flex items-center gap-0.5 border-b">
      <div role="tablist" aria-label="Saved views" className="flex items-center gap-0.5">
      {views.map((v) => {
        const selected = v.id === activeId;
        if (renaming === v.id) {
          return (
            <input key={v.id} autoFocus defaultValue={v.name} aria-label="View name"
              className="mx-1 h-7 rounded-md border bg-card px-2 text-sm"
              onKeyDown={(e) => {
                if (e.key === 'Enter') finish(v.id, e.currentTarget.value, true);
                if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); finish(v.id, '', false); }
              }}
              onBlur={(e) => finish(v.id, e.currentTarget.value, true)} />
          );
        }
        return (
          <span key={v.id} role="presentation" className="flex items-center">
            <button type="button" role="tab" aria-selected={selected} onClick={() => onSelect(v.id)}
              className={`${tab} ${selected ? 'border-foreground font-semibold text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground'}`}>
              {v.name}
              {v.id !== 'all' && counts[v.id] !== undefined && <span className="ml-1 text-xs opacity-80">{counts[v.id]}</span>}
            </button>
            {!v.builtin && (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button type="button" aria-label={`Options for ${v.name}`} className="rounded p-1 text-muted-foreground hover:bg-accent">
                    <MoreHorizontal className="h-3.5 w-3.5" aria-hidden />
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start">
                  <DropdownMenuItem onSelect={() => { done.current = false; setRenaming(v.id); }}>Rename</DropdownMenuItem>
                  <DropdownMenuItem onSelect={() => onDelete(v.id)}>Delete</DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            )}
          </span>
        );
      })}
      </div>
      <button type="button" aria-label="Add a view" onClick={onAdd} className={`${tab} border-transparent text-muted-foreground hover:text-foreground`}>+ View</button>
    </div>
  );
}
