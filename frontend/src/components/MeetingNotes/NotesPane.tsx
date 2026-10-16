'use client';
import { useState } from 'react';
import { ChevronLeft, ChevronRight, X } from 'lucide-react';
import { cleanNoteText, formatRecordingTime, NOTE_MAX_CHARS, sortNotes } from '@/lib/meeting-notes';
import type { MeetingNote } from '@/types';

interface NotesPaneProps {
  notes: MeetingNote[];
  onAdd: (text: string) => void;
  onUpdate: (id: string, text: string) => void;
  onDelete: (id: string) => void;
  onTimeClick?: (at: number) => void;
  collapsed: boolean;
  onToggleCollapsed: () => void;
  disabled?: boolean;
  disabledHint?: string;
}

function NoteRow({ note, onUpdate, onDelete, onTimeClick }: { note: MeetingNote } & Pick<NotesPaneProps, 'onUpdate' | 'onDelete' | 'onTimeClick'>) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(note.text);
  const commit = () => {
    setEditing(false);
    const cleaned = cleanNoteText(draft);
    if (cleaned.ok && cleaned.text !== note.text) onUpdate(note.id, cleaned.text);
    else setDraft(note.text);
  };
  const time = note.at === null ? 'added later' : formatRecordingTime(note.at);
  return (
    <li className="group flex gap-2 py-1.5">
      {note.at !== null && onTimeClick ? (
        <button type="button" className="text-xs text-blue-600 dark:text-blue-400 hover:underline mt-0.5 min-w-[44px] text-left"
          aria-label={`Play from ${time}`} onClick={() => onTimeClick(note.at!)}>{time}</button>
      ) : (
        <span className="text-xs text-muted-foreground mt-0.5 min-w-[44px]">{time}</span>
      )}
      {editing ? (
        <textarea
          autoFocus
          aria-label="Edit note"
          maxLength={NOTE_MAX_CHARS}
          className="flex-1 resize-none rounded border border-input bg-background px-2 py-1 text-sm"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); commit(); }
            else if (e.key === 'Escape') { setDraft(note.text); setEditing(false); }
          }}
        />
      ) : (
        <button type="button" className="flex-1 whitespace-pre-wrap break-words text-left text-sm text-foreground"
          aria-label={`Edit note: ${note.text}`} onClick={() => setEditing(true)}>{note.text}</button>
      )}
      <button type="button" className="opacity-0 group-hover:opacity-100 focus:opacity-100 text-muted-foreground hover:text-foreground"
        aria-label={`Delete note: ${note.text}`} onClick={() => onDelete(note.id)}>
        <X className="h-4 w-4" />
      </button>
    </li>
  );
}

export function NotesPane({ notes, onAdd, onUpdate, onDelete, onTimeClick, collapsed, onToggleCollapsed, disabled, disabledHint }: NotesPaneProps) {
  const [draft, setDraft] = useState('');

  if (collapsed) {
    return (
      <div className="flex w-8 flex-shrink-0 flex-col items-center border-l border-border bg-background pt-3">
        <button type="button" aria-label="Show notes" aria-expanded={false} onClick={onToggleCollapsed}
          className="rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground">
          <ChevronLeft className="h-4 w-4" />
        </button>
      </div>
    );
  }

  const submit = () => {
    const cleaned = cleanNoteText(draft);
    if (!cleaned.ok) return;
    onAdd(cleaned.text);
    setDraft('');
  };

  return (
    <section aria-label="My notes" className="flex min-h-0 min-w-0 flex-1 flex-col bg-background">
      <div className="flex items-center justify-between border-b border-border px-3 py-2">
        <h3 className="text-sm font-medium">My notes ({notes.length})</h3>
        <button type="button" aria-label="Hide notes" aria-expanded={true} onClick={onToggleCollapsed}
          className="rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground">
          <ChevronRight className="h-4 w-4" />
        </button>
      </div>
      <ul className="flex-1 overflow-y-auto px-3">
        {sortNotes(notes).map((n) => (
          <NoteRow key={n.id} note={n} onUpdate={onUpdate} onDelete={onDelete} onTimeClick={onTimeClick} />
        ))}
      </ul>
      <div className="border-t border-border p-2 pb-24">
        <textarea
          aria-label="New note"
          disabled={disabled}
          maxLength={NOTE_MAX_CHARS}
          placeholder={disabled ? disabledHint : 'Type a note, press Enter'}
          className="w-full resize-none rounded-md border border-input bg-background px-3 py-2 text-sm min-h-[60px] disabled:opacity-60"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); submit(); }
          }}
        />
        {disabled && disabledHint && <p className="mt-1 text-xs text-muted-foreground">{disabledHint}</p>}
      </div>
    </section>
  );
}
