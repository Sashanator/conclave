'use client';

import { useRef, useState } from 'react';
import Link from 'next/link';
import { Check, X } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { DestPicker } from '@/components/Tasks/DestPicker';
import { notionDbUrl } from '@/lib/notion';
import { invoke } from '@tauri-apps/api/core';
import { destOf, dueLabel, sendCheck, waitingText, type DestLookup } from '@/lib/tasks-queue';
import type { TaskRow } from '@/lib/tasks';

export type RowActions = {
  busy: boolean;
  send(task: TaskRow): void;
  markSent(task: TaskRow): void;
  setReview(task: TaskRow, review: 'open' | 'keep' | 'drop'): void;
  setDest(task: TaskRow, destId: string | null): void;
  setDue(task: TaskRow, dueDate: string | null): void;
  edit(task: TaskRow, text: string, refocus: boolean): void;
  onKeyDown(e: React.KeyboardEvent<HTMLElement>, task: TaskRow): void;
  refocus(id: string): void;
};

/** One task under review. The row itself is focusable: K/X/1-9/arrows act on it while it has focus. */
export function ReviewRow({ task, lookup, actions }: { task: TaskRow; lookup: DestLookup; actions: RowActions }) {
  const drop = task.review === 'drop';
  const sending = task.review === 'sending';
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const [dueOpen, setDueOpen] = useState(false);
  const editRef = useRef(false); // guards Enter + blur both saving
  const kept = task.review === 'keep' || sending;

  const startEdit = () => { editRef.current = true; setDraft(task.text); setEditing(true); };
  const finish = (save: boolean, refocus: boolean) => {
    if (!editRef.current) return;
    editRef.current = false;
    setEditing(false);
    const text = draft.trim();
    if (save && text && text !== task.text) actions.edit(task, text, refocus);
    else if (refocus) actions.refocus(task.id);
  };
  const dueRef = useRef(false); // true while the date input may still save on blur
  const saveDue = (v: string) => { if ((v || null) !== task.dueDate) actions.setDue(task, v || null); }; // on blur/Enter only: typing a year fires many changes
  const closeDue = () => { setDueOpen(false); actions.refocus(task.id); };

  const check = drop ? null : sendCheck({ ...task, review: 'keep' }, lookup);
  const dest = destOf(task, lookup.dests);
  const waiting = check?.kind === 'waiting' ? check : null;
  const label = `${task.text}${kept ? ', kept' : drop ? ', removed' : ''}`;
  const who = task.owner === 'me' ? 'You' : "Unassigned · AI isn't sure it's yours";
  const btn = 'inline-flex h-7 w-7 items-center justify-center rounded-md border text-xs disabled:opacity-50';

  return (
    <div
      role="group"
      aria-label={label}
      data-row-id={task.id}
      tabIndex={0}
      onKeyDown={(e) => actions.onKeyDown(e, task)}
      className="flex items-start gap-3 rounded-md border border-border px-3 py-2 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
    >
      <div className="flex shrink-0 gap-1">
        <button type="button" disabled={sending} aria-label={`Keep: ${task.text}`} aria-pressed={kept} onClick={() => actions.setReview(task, kept ? 'open' : 'keep')}
          className={`${btn} ${kept ? 'border-emerald-700 bg-emerald-700/10 text-emerald-700 dark:border-emerald-400 dark:text-emerald-400' : 'border-border text-muted-foreground'}`}><Check aria-hidden="true" className="h-3.5 w-3.5" /></button>
        <button type="button" disabled={sending} aria-label={`Remove: ${task.text}`} aria-pressed={drop} onClick={() => actions.setReview(task, drop ? 'open' : 'drop')}
          className={`${btn} ${drop ? 'border-foreground bg-muted text-foreground' : 'border-border text-muted-foreground'}`}><X aria-hidden="true" className="h-3.5 w-3.5" /></button>
      </div>
      <div className="min-w-0 flex-1 space-y-1">
        {editing ? (
          <Input
            autoFocus
            aria-label={`Edit task: ${task.text}`}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={() => finish(true, false)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') { e.preventDefault(); finish(true, true); }
              else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); finish(false, true); }
            }}
          />
        ) : (
          <button type="button" disabled={sending} onClick={startEdit} className={`block text-left text-sm ${drop ? 'text-muted-foreground line-through' : ''}`}>{task.text}</button>
        )}
        <div className="flex flex-wrap items-center gap-x-1 text-xs text-muted-foreground">
          <span>{who}</span>
          <span aria-hidden="true">·</span>
          {dueOpen ? (
            <input
              type="date"
              autoFocus
              aria-label={`Due date for: ${task.text}`}
              defaultValue={task.dueDate ?? ''}
              onBlur={(e) => { if (dueRef.current) saveDue(e.currentTarget.value); dueRef.current = false; setDueOpen(false); }}
              onKeyDown={(e) => {
                if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); dueRef.current = false; closeDue(); }
                else if (e.key === 'Enter') { e.preventDefault(); saveDue(e.currentTarget.value); dueRef.current = false; closeDue(); }
              }}
              className="rounded border border-input bg-transparent px-1 text-xs"
            />
          ) : (
            <button type="button" aria-label={`Due date: ${dueLabel(task)}. Change`} onClick={() => { dueRef.current = true; setDueOpen(true); }} className="underline-offset-2 hover:underline">{dueLabel(task)}</button>
          )}
          {drop && <span>· removed by {task.removedBy === 'ai' ? 'AI' : 'you'}</span>}
          {sending && <span>· Sending…</span>}
        </div>
        {!drop && !sending && (
          <div className="flex flex-wrap items-center gap-2">
            <DestPicker task={task} dests={lookup.dests} onPick={(destId) => actions.setDest(task, destId)} />
            {task.sendAmbiguous ? (
              <span className="flex flex-wrap items-center gap-2 text-xs text-amber-700 dark:text-amber-400">
                May already be in Notion. Check the database before sending again.
                {dest && <button type="button" className="underline" onClick={() => void invoke('open_external_url', { url: notionDbUrl(dest.databaseId) })}>Open Notion database</button>}
                <button type="button" disabled={actions.busy} className="underline disabled:opacity-50" onClick={() => actions.markSent(task)}>Mark as sent</button>
                <button type="button" disabled={actions.busy} className="underline disabled:opacity-50" onClick={() => actions.send(task)}>Send again</button>
              </span>
            ) : task.sendError ? (
              <span className="flex flex-wrap items-center gap-2 text-xs text-red-600 dark:text-red-400">
                Not sent: {task.sendError}
                <button type="button" disabled={actions.busy} className="underline disabled:opacity-50" onClick={() => actions.send(task)}>Retry</button>
              </span>
            ) : waiting && (
              <span className="text-xs text-amber-700 dark:text-amber-400">
                {waitingText(waiting.dest.name, waiting.state)} <Link href="/settings?tab=notion" className="underline">Fix in Settings</Link>
              </span>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
