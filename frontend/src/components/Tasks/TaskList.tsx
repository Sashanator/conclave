'use client';

import { useEffect, useRef, useState } from 'react';
import Link from 'next/link';
import { invoke } from '@tauri-apps/api/core';
import { Clock, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { meetingHref } from '@/lib/meeting-target';
import { groupByMeeting, type TaskRow } from '@/lib/tasks';

type Status = 'open' | 'done';
type Run = (command: string, args: Record<string, unknown>) => Promise<boolean>;

const SAVE_FAILED = 'Could not save that change. The list has been refreshed.';

function formatDate(date: string): string {
  const d = new Date(date);
  return Number.isNaN(d.getTime()) ? date : d.toLocaleDateString();
}

export function TaskList() {
  const [status, setStatus] = useState<Status>('open');
  const [rows, setRows] = useState<TaskRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [newText, setNewText] = useState('');

  // The list is reloaded after every change, so it shows what the backend holds. A reload that
  // finishes after the tab changed is dropped, so it cannot show the old tab's tasks.
  const tabRef = useRef<Status>('open');
  const reload = async () => {
    const tab = tabRef.current;
    const list = await invoke<TaskRow[]>('tasks_list', { status: tab });
    if (tab === tabRef.current) setRows(list);
  };
  const choose = (next: Status) => {
    tabRef.current = next;
    setStatus(next);
  };

  useEffect(() => {
    let live = true;
    invoke<TaskRow[]>('tasks_list', { status })
      .then((list) => {
        if (live) setRows(list);
      })
      .catch((err) => {
        console.error('Tasks failed to load:', err);
        if (live) setError('Could not load tasks.');
      });
    return () => {
      live = false;
    };
  }, [status]);

  const run: Run = async (command, args) => {
    setError(null);
    let ok = true;
    try {
      await invoke(command, args);
    } catch {
      // The error text can echo task content, so it is not logged
      console.error(`${command} failed`);
      ok = false;
      setError(SAVE_FAILED);
    }
    try {
      await reload();
    } catch {
      setError('Could not load tasks.');
    }
    return ok;
  };

  const add = async () => {
    const text = newText.trim();
    if (!text) return;
    if (await run('tasks_add', { text })) setNewText('');
  };

  const groups = groupByMeeting(rows);

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-2">
        <Button variant={status === 'open' ? 'secondary' : 'ghost'} aria-pressed={status === 'open'} onClick={() => choose('open')}>
          Open
        </Button>
        <Button variant={status === 'done' ? 'secondary' : 'ghost'} aria-pressed={status === 'done'} onClick={() => choose('done')}>
          Done
        </Button>
      </div>

      {status === 'open' && (
        <form
          className="flex items-center gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            add();
          }}
        >
          <Input
            aria-label="New task text"
            value={newText}
            onChange={(e) => setNewText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                add();
              }
            }}
          />
          <Button type="submit" variant="outline">
            Add task
          </Button>
        </form>
      )}

      {error && <p role="alert" className="text-sm text-red-600">{error}</p>}

      {groups.map((group) => (
        <section key={group.key} className="space-y-2">
          <header className="flex items-baseline gap-2">
            {group.meetingId ? (
              <Link href={meetingHref({ id: group.meetingId })} className="font-medium hover:underline">
                {group.title}
              </Link>
            ) : (
              <h2 className="font-medium">{group.title}</h2>
            )}
            {group.date && <span className="text-sm text-muted-foreground">{formatDate(group.date)}</span>}
          </header>
          <ul className="divide-y divide-border rounded-lg border border-border bg-card">
            {group.tasks.map((task) => (
              <TaskItem key={task.id} task={task} run={run} />
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

function TaskItem({ task, run }: { task: TaskRow; run: Run }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(task.text);
  const done = task.status === 'done';

  // A blank edit only ends the edit: the backend rejects an empty task
  const save = () => {
    setEditing(false);
    const text = draft.trim();
    if (text && text !== task.text) run('tasks_edit', { id: task.id, text });
  };

  return (
    <li className="flex flex-wrap items-center gap-3 px-4 py-2">
      <input
        type="checkbox"
        className="h-4 w-4"
        checked={done}
        aria-label={`Mark ${done ? 'open' : 'done'}: ${task.text}`}
        onChange={() => run('tasks_set_status', { id: task.id, status: done ? 'open' : 'done' })}
      />

      {editing ? (
        <Input
          aria-label="Edit task text"
          className="flex-1"
          autoFocus
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') save();
            if (e.key === 'Escape') {
              setDraft(task.text);
              setEditing(false);
            }
          }}
        />
      ) : (
        <button
          type="button"
          className={`flex-1 text-left ${done ? 'text-muted-foreground line-through' : ''}`}
          onClick={() => {
            setDraft(task.text);
            setEditing(true);
          }}
        >
          {task.text}
        </button>
      )}

      {task.due && <span className="text-sm text-muted-foreground">{task.due}</span>}

      {task.owner === 'unassigned' && (
        <span className="flex items-center gap-1 text-sm">
          <span className="rounded-full border border-border px-2 py-0.5 text-xs text-muted-foreground">unassigned</span>
          <Button variant="ghost" size="sm" onClick={() => run('tasks_claim', { id: task.id })}>
            Mine
          </Button>
          <Button variant="ghost" size="sm" onClick={() => run('tasks_set_status', { id: task.id, status: 'dismissed' })}>
            Dismiss
          </Button>
        </span>
      )}

      {task.at !== null && task.meetingId && (
        <Link
          href={meetingHref({ id: task.meetingId, t: task.at })}
          aria-label="Show in transcript"
          className="text-muted-foreground hover:text-foreground"
        >
          <Clock className="h-4 w-4" />
        </Link>
      )}

      <Button variant="ghost" size="icon" aria-label="Delete task" onClick={() => run('tasks_delete', { id: task.id })}>
        <Trash2 className="h-4 w-4" />
      </Button>
    </li>
  );
}
