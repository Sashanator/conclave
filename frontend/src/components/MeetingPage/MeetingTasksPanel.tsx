'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { setReviewed } from '@/lib/meetings';
import { splitTasks, taskKeyAction } from '@/lib/meeting-page';
import type { TaskRow } from '@/lib/tasks';

const STATUSES = ['open', 'done', 'dismissed'] as const;

/** "Tasks from this meeting": kept rows on top, removed ones below with Restore. Failed edits toast and roll back. */
export function MeetingTasksPanel({ meetingId, reviewed: initialReviewed = false }: { meetingId: string; reviewed?: boolean }) {
  const [tasks, setTasks] = useState<TaskRow[]>([]);
  const [reviewed, setReviewedState] = useState(initialReviewed);
  const rows = useRef<(HTMLElement | null)[]>([]);
  useEffect(() => setReviewedState(initialReviewed), [initialReviewed]);

  useEffect(() => {
    let active = true;
    Promise.all(STATUSES.map((status) => invoke<TaskRow[]>('tasks_list', { status })))
      .then((lists) => { if (active) setTasks(lists.flat()); })
      .catch(() => { if (active) toast.error('Could not load the tasks.'); });
    return () => { active = false; };
  }, [meetingId]);

  const { kept, removed } = splitTasks(tasks, meetingId);
  const all = [...kept, ...removed];

  const edit = useCallback(async (task: TaskRow, patch: Partial<TaskRow>, calls: [string, Record<string, unknown>][]) => {
    const before = tasks;
    setTasks((cur) => cur.map((t) => (t.id === task.id ? { ...t, ...patch } : t)));
    try {
      for (const [command, args] of calls) await invoke(command, args);
    } catch {
      setTasks(before);
      toast.error('Could not save the task change.');
    }
  }, [tasks]);

  const keep = (task: TaskRow) => {
    if (task.owner === 'me' && task.status !== 'dismissed') return Promise.resolve();
    const calls: [string, Record<string, unknown>][] = [['tasks_claim', { id: task.id }]];
    if (task.status === 'dismissed') calls.push(['tasks_set_status', { id: task.id, status: 'open' }]);
    return edit(task, { owner: 'me', status: task.status === 'dismissed' ? 'open' : task.status }, calls);
  };
  const remove = (task: TaskRow) =>
    task.status === 'dismissed' ? Promise.resolve() : edit(task, { status: 'dismissed' }, [['tasks_set_status', { id: task.id, status: 'dismissed' }]]);

  const toggleReviewed = async () => {
    const next = !reviewed;
    setReviewedState(next);
    try { await setReviewed(meetingId, next); } catch {
      setReviewedState(!next);
      toast.error('Could not save the review state.');
    }
  };

  const onKey = (e: React.KeyboardEvent, task: TaskRow, index: number) => {
    const action = taskKeyAction(e);
    if (!action) return;
    e.preventDefault();
    if (action === 'keep') void keep(task);
    else if (action === 'remove') void remove(task);
    else rows.current[index + (action === 'up' ? -1 : 1)]?.focus();
  };

  const row = (task: TaskRow, index: number, isRemoved: boolean) => (
    <li
      key={task.id}
      ref={(el) => { rows.current[index] = el; }}
      tabIndex={0}
      onKeyDown={(e) => onKey(e, task, index)}
      className="flex items-start justify-between gap-2 rounded-md border border-border px-3 py-2 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
    >
      <div className="min-w-0">
        <div className={isRemoved ? 'text-sm text-muted-foreground' : 'text-sm'}>{task.text}</div>
        <div className="text-xs text-muted-foreground">
          {isRemoved ? (task.owner === 'unassigned' ? 'removed by AI' : 'removed') : `You${task.due ? ` · by ${task.due}` : ''}`}
        </div>
      </div>
      {isRemoved ? (
        <Button variant="ghost" size="sm" onClick={() => keep(task)}>Restore</Button>
      ) : (
        <Button variant="ghost" size="sm" aria-label="Remove task" onClick={() => remove(task)}>✕</Button>
      )}
    </li>
  );

  return (
    <aside aria-label="Tasks from this meeting" className="space-y-3 p-3">
      <h2 className="text-sm font-medium">Tasks from this meeting <small aria-live="polite" className="text-muted-foreground">{kept.length} kept</small></h2>
      <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        <Button variant="outline" size="sm" onClick={() => kept.filter((t) => t.owner !== 'me').forEach((t) => void keep(t))}>Keep all mine</Button>
        <span>K keep · X remove · ↑↓ move</span>
      </div>
      {kept.length > 0 && <ul className="space-y-2">{kept.map((t, i) => row(t, i, false))}</ul>}
      {removed.length > 0 && (
        <>
          <h3 className="text-xs font-medium text-muted-foreground">Removed ({removed.length})</h3>
          <ul className="space-y-2">{removed.map((t, i) => row(t, kept.length + i, true))}</ul>
        </>
      )}
      {all.length === 0 && <p className="text-sm text-muted-foreground">No tasks yet.</p>}
      <div className="flex items-center justify-between gap-2 border-t border-border pt-3">
        <small className="text-muted-foreground">Removed tasks can be restored.</small>
        <Button variant={reviewed ? 'outline' : 'default'} size="sm" aria-pressed={reviewed} onClick={toggleReviewed}>
          {reviewed ? 'Reviewed ✓' : 'Done reviewing'}
        </Button>
      </div>
    </aside>
  );
}
