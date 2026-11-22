'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { MeetingCostLine } from '@/components/MeetingDetails/MeetingCostLine';
import { setReviewed } from '@/lib/meetings';
import { DestPicker } from '@/components/Tasks/DestPicker';
import type { Destination, NotionConfig } from '@/lib/notion';
import { splitTasks, taskKeyAction } from '@/lib/meeting-page';
import type { TaskRow } from '@/lib/tasks';
import { TASKS_CHANGED_EVENT } from '@/lib/tasks-queue';

/** "Tasks from this meeting": kept rows on top, removed ones below with Restore. Failed edits toast and roll back. */
export function MeetingTasksPanel({ meetingId, reviewed: initialReviewed = false, onCountChange, costRefreshKey }: { meetingId: string; reviewed?: boolean; onCountChange?: (kept: number) => void; costRefreshKey?: string }) {
  const [tasks, setTasks] = useState<TaskRow[]>([]);
  const [dests, setDests] = useState<Destination[]>([]);
  const [reviewed, setReviewedState] = useState(initialReviewed);
  const rows = useRef<(HTMLElement | null)[]>([]);
  useEffect(() => setReviewedState(initialReviewed), [initialReviewed]);

  useEffect(() => {
    let active = true;
    const load = () => invoke<TaskRow[]>('tasks_queue_list')
      .then((list) => { if (active) setTasks(list); })
      .catch(() => { console.error('tasks_queue_list failed'); if (active) toast.error('Could not load the tasks.'); }); // command name only: the error text can carry task content
    void load();
    let off: (() => void) | null = null;
    listen(TASKS_CHANGED_EVENT, () => void load()).then((u) => { if (active) off = u; else u(); }).catch(() => console.error('tasks-changed listen failed'));
    // No destinations is a quiet state: the pickers then say "Choose database…".
    invoke<NotionConfig>('notion_get_config').then((c) => { if (active) setDests(c?.destinations ?? []); }).catch(() => {});
    return () => { active = false; off?.(); };
  }, [meetingId]);

  const { kept, removed } = splitTasks(tasks, meetingId);
  const all = [...kept, ...removed];
  useEffect(() => { onCountChange?.(kept.length); }, [kept.length, onCountChange]);

  // Each step carries the change it makes: a failure rolls back only this task, to the steps that did save.
  type Step = [string, Record<string, unknown>, Partial<TaskRow>];
  const edit = useCallback(async (task: TaskRow, steps: Step[]) => {
    const patchTask = (patch: Partial<TaskRow>) => setTasks((cur) => cur.map((t) => (t.id === task.id ? { ...t, ...patch } : t)));
    patchTask(Object.assign({}, ...steps.map((s) => s[2])));
    let saved: Partial<TaskRow> = {};
    try {
      for (const [command, args, patch] of steps) { await invoke(command, args); saved = { ...saved, ...patch }; }
    } catch {
      console.error(`${steps.map((x) => x[0]).join(', ')} failed`);
      // Only the fields this edit changed and did not save: a later edit to the same task stays.
      const undo: Record<string, unknown> = {};
      for (const [, , patch] of steps) for (const k of Object.keys(patch)) if (!(k in saved)) undo[k] = task[k as keyof TaskRow];
      patchTask(undo as Partial<TaskRow>);
      toast.error('Could not save the task change.');
    }
  }, []);

  const keep = (task: TaskRow) =>
    ['keep', 'sending', 'sent'].includes(task.review) ? Promise.resolve() : edit(task, [['tasks_set_review', { id: task.id, review: 'keep' }, { review: 'keep', owner: 'me' }]]);
  const remove = (task: TaskRow) =>
    ['drop', 'sending', 'sent'].includes(task.review) ? Promise.resolve() : edit(task, [['tasks_set_review', { id: task.id, review: 'drop' }, { review: 'drop', removedBy: 'user' }]]);

  const toggleReviewed = async () => {
    const next = !reviewed;
    setReviewedState(next);
    try { await setReviewed(meetingId, next); } catch {
      console.error('set_reviewed failed');
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
          {isRemoved ? (task.removedBy === 'ai' ? 'removed by AI' : 'removed') : `${task.owner === 'me' ? 'You' : 'Unassigned'}${task.due ? ` · by ${task.due}` : ''}`}
        </div>
        {task.review === 'keep' && <div className="mt-1"><DestPicker task={task} dests={dests} onPick={(destId) => edit(task, [['tasks_set_dest', { id: task.id, destId }, { destId }]])} /></div>}
        {task.review === 'sending' && <div className="mt-1 text-xs text-muted-foreground">Sending…</div>}
        {task.review === 'sent' && <div className="mt-1 text-xs text-muted-foreground">Sent to Notion</div>}
      </div>
      {task.review !== 'sent' && (
        <div className="flex shrink-0 gap-1">
          <Button variant="ghost" size="sm" aria-pressed={['keep', 'sending'].includes(task.review)} onClick={() => keep(task)}>Keep <kbd className="ml-1 text-xs text-muted-foreground">K</kbd></Button>
          <Button variant="ghost" size="sm" aria-pressed={isRemoved} onClick={() => remove(task)}>Remove <kbd className="ml-1 text-xs text-muted-foreground">X</kbd></Button>
        </div>
      )}
    </li>
  );

  return (
    <aside aria-label="Tasks from this meeting" className="space-y-3 p-3">
      <h2 className="text-base font-semibold">Tasks from this meeting <small aria-live="polite" className="text-muted-foreground">{all.filter((t) => ['keep', 'sending', 'sent'].includes(t.review)).length} kept</small></h2>
      <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        <span>Keyboard: <kbd>K</kbd> keep · <kbd>X</kbd> remove · <kbd>↑↓</kbd> move</span>
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
      <div className="border-t border-border pt-1"><MeetingCostLine meetingId={meetingId} refreshKey={costRefreshKey} /></div>
    </aside>
  );
}
