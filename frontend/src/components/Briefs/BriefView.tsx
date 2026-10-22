'use client';

import { useEffect, useState } from 'react';
import Link from 'next/link';
import { invoke } from '@tauri-apps/api/core';
import { meetingHref } from '@/lib/meeting-target';
import type { TaskRow } from '@/lib/tasks';

export type BriefContent = {
  eventTitle: string;
  eventStart: string;
  templateName: string;
  previousMeetingId: string;
  previousTitle: string;
  previousDate: string;
  requests: string[];
  decisions: string[];
  openTasks: TaskRow[];
};

const SAVE_FAILED = 'Could not save that change.';

// Shown as given when it is not a date, so the text is never lost.
function formatWhen(value: string): string {
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? value : d.toLocaleString();
}

function TextList({ items }: { items: string[] }) {
  if (items.length === 0) return <p className="text-sm text-muted-foreground">None</p>;
  return (
    <ul className="list-disc space-y-1 pl-5">
      {items.map((item, i) => (
        <li key={i}>{item}</li>
      ))}
    </ul>
  );
}

export function BriefView({ id }: { id: string }) {
  const [brief, setBrief] = useState<BriefContent | null>(null);
  const [state, setState] = useState<'loading' | 'ready' | 'missing' | 'error'>('loading');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setState('loading');
    invoke<BriefContent | null>('brief_get', { id })
      .then((content) => {
        if (!live) return;
        setBrief(content);
        setState(content ? 'ready' : 'missing');
      })
      .catch((err) => {
        console.error('Brief failed to load:', err);
        if (live) setState('error');
      });
    return () => {
      live = false;
    };
  }, [id]);

  // A ticked task is no longer open, so it leaves this list.
  const tick = async (task: TaskRow) => {
    setError(null);
    try {
      await invoke('tasks_set_status', { id: task.id, status: 'done' });
      setBrief((current) => current && { ...current, openTasks: current.openTasks.filter((t) => t.id !== task.id) });
    } catch {
      // The error text can echo task content, so it is not logged
      console.error('tasks_set_status failed');
      setError(SAVE_FAILED);
    }
  };

  if (state === 'loading') return <p className="text-sm text-muted-foreground">Loading…</p>;
  if (state === 'error') return <p role="alert" className="text-sm text-red-600">Could not load this brief.</p>;
  if (state === 'missing' || !brief) return <p className="text-sm text-muted-foreground">This brief is no longer there.</p>;

  return (
    <article className="space-y-6 rounded-lg border border-border bg-card p-6">
      <header className="space-y-1">
        <h2 className="text-2xl font-semibold">{brief.eventTitle}</h2>
        <p className="text-sm text-muted-foreground">
          {formatWhen(brief.eventStart)} · {brief.templateName}
        </p>
      </header>

      <section aria-label="From last time" className="space-y-1">
        <h3 className="font-medium">From last time</h3>
        <p>
          <Link href={meetingHref({ id: brief.previousMeetingId })} className="hover:underline">
            {brief.previousTitle}
          </Link>{' '}
          <span className="text-sm text-muted-foreground">{formatWhen(brief.previousDate)}</span>
        </p>
      </section>

      <section className="space-y-1">
        <h3 className="font-medium">Requests</h3>
        <TextList items={brief.requests} />
      </section>

      <section className="space-y-1">
        <h3 className="font-medium">Decisions</h3>
        <TextList items={brief.decisions} />
      </section>

      <section className="space-y-2">
        <h3 className="font-medium">Open tasks</h3>
        {error && <p role="alert" className="text-sm text-red-600">{error}</p>}
        {brief.openTasks.length === 0 ? (
          <p className="text-sm text-muted-foreground">None</p>
        ) : (
          <ul className="divide-y divide-border">
            {brief.openTasks.map((task) => (
              <li key={task.id} className="flex items-center gap-3 py-2">
                <input
                  type="checkbox"
                  className="h-4 w-4"
                  checked={false}
                  aria-label={`Mark done: ${task.text}`}
                  onChange={() => tick(task)}
                />
                <span>{task.text}</span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </article>
  );
}
