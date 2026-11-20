'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import Link from 'next/link';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { format } from 'date-fns';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { ReviewRow, type RowActions } from '@/components/Tasks/ReviewRow';
import { SendBar, type SendResult } from '@/components/Tasks/SendBar';
import { formatRowDate } from '@/lib/dates';
import { rememberOrigin } from '@/lib/back-nav';
import { meetingHref } from '@/lib/meeting-target';
import { parseDbTime } from '@/lib/meetings';
import { taskKeyAction } from '@/lib/meeting-page';
import type { Destination, NotionConfig, SendReport } from '@/lib/notion';
import type { TaskRow } from '@/lib/tasks';
import { TASKS_CHANGED_EVENT, destForKey, groupQueue, isEditingText, isToReview, pickFocus, resultLine, sendSummary, toReviewCount, type QueueGroup } from '@/lib/tasks-queue';

type Tab = 'review' | 'sent' | 'removed';
type Call = [string, Record<string, unknown>];
const dim = 'text-muted-foreground';

function Chip({ templateId, templates }: { templateId: string | null; templates: { id: string; name: string }[] }) {
  const i = templates.findIndex((t) => t.id === templateId);
  return (
    <span className="inline-block max-w-full truncate whitespace-nowrap rounded-full px-2 align-middle text-xs" style={{ background: `hsl(var(--cat-${i >= 0 ? (i % 4) + 1 : 5}) / .12)` }}>
      {i >= 0 ? templates[i].name : 'No template'}
    </span>
  );
}

export function TasksQueue() {
  const [tasks, setTasks] = useState<TaskRow[]>([]);
  const [config, setConfig] = useState<NotionConfig | null>(null);
  const [templates, setTemplates] = useState<{ id: string; name: string }[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [tab, setTab] = useState<Tab>('review');
  const [shown, setShown] = useState<Set<string>>(new Set());
  const [tick, setTick] = useState(0);
  const [sending, setSending] = useState(false);
  const [result, setResult] = useState<SendResult | null>(null);
  const sendingRef = useRef(false); // state lags a same-tick second click; the ref does not
  const root = useRef<HTMLDivElement>(null);
  const pending = useRef<{ before: string[]; wanted: string; soft?: boolean } | null>(null);

  const reload = useCallback(async () => {
    try {
      const [list, cfg] = await Promise.all([invoke<TaskRow[]>('tasks_queue_list'), invoke<NotionConfig>('notion_get_config')]);
      setTasks(list ?? []);
      setConfig(cfg ?? null);
      setError(null);
    } catch (e) {
      console.error('tasks_queue_list failed'); // name only: the error text can carry task content
      setError('Could not load tasks.');
    }
    setLoaded(true);
  }, []);

  useEffect(() => {
    void reload();
    invoke<{ templates: { id: string; name: string }[] }>('api_list_templates').then((r) => setTemplates(r?.templates ?? [])).catch(() => console.error('api_list_templates failed'));
    let off: (() => void) | null = null;
    let gone = false;
    listen(TASKS_CHANGED_EVENT, () => void reload()).then((u) => { if (gone) u(); else off = u; }).catch(() => console.error('tasks-changed listen failed'));
    return () => { gone = true; off?.(); };
  }, [reload]);

  const ids = () => Array.from(root.current?.querySelectorAll<HTMLElement>('[data-row-id]') ?? []).map((el) => el.dataset.rowId!);
  const request = (before: string[], wanted: string) => { pending.current = { before, wanted }; setTick((n) => n + 1); };

  // After every render that follows an action: put focus on a row (or the first tab) unless it is already inside the wanted one.
  useEffect(() => {
    const p = pending.current;
    if (!p) return;
    pending.current = null;
    if (p.soft && document.activeElement && document.activeElement !== document.body) return; // blur-save: focus went elsewhere on purpose
    const after = ids();
    const id = pickFocus(p.before, after, p.wanted);
    const el = id ? Array.from(root.current?.querySelectorAll<HTMLElement>('[data-row-id]') ?? []).find((r) => r.dataset.rowId === id) : null;
    if (el) { if (!el.contains(document.activeElement)) el.focus(); }
    else root.current?.querySelector<HTMLElement>('#tab-review')?.focus();
  }, [tick, tasks]);

  const run = async (calls: Call[], wanted: string, soft = false) => {
    const before = ids();
    pending.current = { before, wanted, soft };
    try {
      for (const [c, a] of calls) await invoke(c, a);
    } catch (e) {
      console.error(`${calls.map(([c]) => c).join(', ')} failed`);
      toast.error('Could not save that change.');
    }
    await reload();
    setTick((n) => n + 1); // also when the reload failed and tasks kept their identity
  };
  const act = (command: string, args: Record<string, unknown>, wanted: string) => run([[command, args]], wanted);

  const sendRef = useRef<(ids: string[], resendAmbiguous?: boolean) => void>(() => {});
  const send = async (sendIds: string[], resendAmbiguous = false) => {
    if (sendingRef.current || sendIds.length === 0) return;
    sendingRef.current = true;
    setSending(true);
    setResult(null);
    const before = ids();
    try {
      setResult(resultLine(await invoke<SendReport>('notion_send', { taskIds: sendIds, resendAmbiguous })));
    } catch (e) {
      console.error('Send failed:', e);
      setResult({ sent: null, notSent: null, error: e instanceof Error ? e.message : String(e) });
    } finally {
      sendingRef.current = false;
      setSending(false);
      await reload();
      request(before, before[0] ?? '');
    }
  };
  sendRef.current = send;
  useEffect(() => {
    if (!result) return;
    const t = setTimeout(() => setResult(null), 8000);
    return () => clearTimeout(t);
  }, [result]);

  const dests: Destination[] = config?.destinations ?? [];
  const lookup = { dests, tokenSet: !!config?.tokenLast4 };
  const sortedDests = [...dests].sort((a, b) => a.position - b.position);

  const actions: RowActions = {
    busy: sending,
    send: (t) => void send([t.id], t.sendAmbiguous), // only a row flagged ambiguous is resent, and only by this explicit click
    markSent: (t) => void act('tasks_mark_sent', { id: t.id }, t.id),
    setReview: (t, review) => void act('tasks_set_review', { id: t.id, review }, t.id),
    setDest: (t, destId) => void act('tasks_set_dest', { id: t.id, destId }, t.id),
    setDue: (t, dueDate) => void act('tasks_set_due_date', { id: t.id, dueDate }, t.id),
    edit: (t, text, refocus) => void (refocus ? act('tasks_edit', { id: t.id, text }, t.id) : run([['tasks_edit', { id: t.id, text }]], t.id, true)),
    refocus: (id) => request(ids(), id),
    onKeyDown: (e, t) => {
      if (e.target !== e.currentTarget || isEditingText(document.activeElement)) return;
      const before = ids();
      const i = before.indexOf(t.id);
      const action = taskKeyAction(e);
      if (action === 'up' || action === 'down') {
        e.preventDefault();
        const next = before[i + (action === 'up' ? -1 : 1)];
        if (next) Array.from(root.current!.querySelectorAll<HTMLElement>('[data-row-id]')).find((r) => r.dataset.rowId === next)?.focus();
      } else if (action) {
        e.preventDefault();
        if (t.review === 'sending') return;
        void act('tasks_set_review', { id: t.id, review: action === 'keep' ? 'keep' : 'drop' }, before[i + 1] ?? t.id);
      } else if (t.review !== 'drop' && t.review !== 'sending' && !e.ctrlKey && !e.metaKey && !e.altKey) {
        const d = destForKey(e.key, dests);
        if (d) { e.preventDefault(); void act('tasks_set_dest', { id: t.id, destId: d.id }, t.id); }
      }
    },
  };

  const keepAll = (g: QueueGroup) => {
    const mine = g.tasks.filter((t) => t.review === 'open' && t.owner === 'me');
    void run(mine.map((t): Call => ['tasks_set_review', { id: t.id, review: 'keep' }]), (g.tasks.find((t) => t.review !== 'drop') ?? g.tasks[0]).id);
  };

  const sent = tasks.filter((t) => t.review === 'sent');
  const summary = sendSummary(tasks, lookup);
  const sumRef = useRef(summary);
  sumRef.current = summary;
  useEffect(() => {
    if (tab !== 'review') return;
    const h = (e: KeyboardEvent) => {
      if (!(e.ctrlKey && e.key === 'Enter') || isEditingText(e.target as Element | null)) return;
      e.preventDefault();
      sendRef.current(sumRef.current.sendableIds);
    };
    window.addEventListener('keydown', h);
    return () => window.removeEventListener('keydown', h);
  }, [tab]);

  const removed = tasks.filter((t) => t.review === 'drop');
  const nReview = toReviewCount(tasks);
  const reviewGroups = groupQueue(tasks.filter((t) => t.review !== 'sent')).filter((g) => g.tasks.some(isToReview));

  const tabBtn = (id: Tab, label: string, n: number | '') => (
    <button
      key={id}
      type="button"
      role="tab"
      id={`tab-${id}`}
      aria-selected={tab === id}
      aria-controls={`panel-${id}`}
      tabIndex={tab === id ? 0 : -1}
      onClick={() => setTab(id)}
      onKeyDown={(e) => {
        const d = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
        if (!d) return;
        e.preventDefault();
        const order: Tab[] = ['review', 'sent', 'removed'];
        const next = order[(order.indexOf(id) + d + order.length) % order.length];
        setTab(next);
        document.getElementById(`tab-${next}`)?.focus();
      }}
      className={`border-b-2 px-3 py-2 text-sm ${tab === id ? 'border-primary font-medium' : `border-transparent ${dim}`}`}
    >
      {label} <small>{n}</small>
    </button>
  );

  const header = (g: QueueGroup) => (
    <div className="flex flex-wrap items-center gap-2">
      {g.meetingId ? (
        <Link href={meetingHref({ id: g.meetingId })} onClick={() => rememberOrigin({ path: '/tasks', label: 'Tasks' })} className="text-sm font-medium hover:underline">{g.title}</Link>
      ) : <span className="text-sm font-medium">{g.title}</span>}
      {g.date && <span className={`text-xs ${dim}`}>{formatRowDate(parseDbTime(g.date))}</span>}
      <Chip templateId={g.templateId} templates={templates} />
    </div>
  );

  return (
    <>
      <div ref={root} className="min-h-0 flex-1 overflow-y-auto">
        <p className={`mb-3 text-sm ${dim}`}>Tasks the AI found in your meetings. Keep the ones you want, then send them to Notion.</p>
        <div role="tablist" aria-label="Task lists" className="mb-3 flex border-b border-border">
          {tabBtn('review', 'To review', nReview || '')}
          {tabBtn('sent', 'Sent to Notion', sent.length)}
          {tabBtn('removed', 'Removed', removed.length)}
        </div>
        {error && <p role="alert" className="text-sm text-destructive">{error}</p>}

        <div role="tabpanel" id="panel-review" aria-labelledby="tab-review" hidden={tab !== 'review'} className="space-y-4">
          {tab === 'review' && loaded && (
            <>
              <p className={`flex flex-wrap gap-x-3 text-xs ${dim}`}>
                <span><kbd>K</kbd> keep</span><span><kbd>X</kbd> remove</span>
                {sortedDests.map((d, i) => i < 9 && <span key={d.id}><kbd>{i + 1}</kbd> {d.name}</span>)}
                <span><kbd>↑</kbd><kbd>↓</kbd> move</span><span><kbd>Ctrl</kbd>+<kbd>Enter</kbd> send</span><span>Click a task&apos;s text to edit it</span>
              </p>
              {reviewGroups.length === 0 && !error && (
                <div className="py-8 text-center"><p className="font-medium">All reviewed</p><p className={`text-sm ${dim}`}>New tasks appear here after each meeting is summarized.</p></div>
              )}
              {reviewGroups.map((g) => {
                const drops = g.tasks.filter((t) => t.review === 'drop');
                const open = shown.has(g.key);
                const live = g.tasks.filter((t) => t.review !== 'drop');
                return (
                  <section key={g.key} aria-label={g.title} className="space-y-2">
                    <div className="flex items-center justify-between gap-2">
                      {header(g)}
                      {g.tasks.some((t) => t.review === 'open' && t.owner === 'me') && <Button variant="outline" size="sm" onClick={() => keepAll(g)}>Keep all mine</Button>}
                    </div>
                    {live.map((t) => <ReviewRow key={t.id} task={t} lookup={lookup} actions={actions} />)}
                    {drops.length > 0 && (
                      <>
                        <p className={`text-xs ${dim}`}>
                          {drops.length} removed{drops.some((t) => t.removedBy === 'ai') ? " (AI thinks they aren't yours)" : ''} ·{' '}
                          <button type="button" aria-expanded={open} className="underline" onClick={() => setShown((s) => { const n = new Set(s); if (open) n.delete(g.key); else n.add(g.key); return n; })}>{open ? 'Hide' : 'Show'}</button>
                        </p>
                        {open && drops.map((t) => <ReviewRow key={t.id} task={t} lookup={lookup} actions={actions} />)}
                      </>
                    )}
                  </section>
                );
              })}
            </>
          )}
        </div>

        <div role="tabpanel" id="panel-sent" aria-labelledby="tab-sent" hidden={tab !== 'sent'} className="space-y-4">
          {tab === 'sent' && (
            <>
              {groupQueue(sent).map((g) => (
                <section key={g.key} aria-label={g.title} className="space-y-2">
                  {header(g)}
                  {g.tasks.map((t) => {
                    const d = dests.find((x) => x.id === t.destId);
                    return (
                      <div key={t.id} role="group" aria-label={t.text} data-row-id={t.id} tabIndex={0} className="flex flex-wrap items-center gap-x-3 rounded-md border border-border px-3 py-2 text-sm">
                        <span className="min-w-0 flex-1">{t.text}</span>
                        <span className={`text-xs ${dim}`}>{t.owner === 'me' ? 'You' : 'Unassigned'}</span>
                        {d && <span className="rounded-full border border-border px-2 text-xs">{d.name}</span>}
                        {t.sentAt && <span className="text-xs text-emerald-700 dark:text-emerald-400">Sent {format(parseDbTime(t.sentAt), 'EEE d MMM, HH:mm')}</span>}
                        {t.notionUrl && <button type="button" className="text-xs underline" onClick={() => void invoke('open_external_url', { url: t.notionUrl }).catch(() => toast.error('Could not open Notion.'))}>Open in Notion ↗</button>}
                      </div>
                    );
                  })}
                </section>
              ))}
              <p className={`text-xs ${dim}`}>Conclave doesn&apos;t follow tasks once they are in Notion. Tick them off there.</p>
            </>
          )}
        </div>

        <div role="tabpanel" id="panel-removed" aria-labelledby="tab-removed" hidden={tab !== 'removed'} className="space-y-2">
          {tab === 'removed' && (
            <>
              {removed.length === 0 && <p className={`text-sm ${dim}`}>Nothing removed.</p>}
              {groupQueue(removed).flatMap((g) => g.tasks.map((t) => (
                <div key={t.id} role="group" aria-label={t.text} data-row-id={t.id} tabIndex={0} className="flex items-center justify-between gap-3 rounded-md border border-border px-3 py-2">
                  <div className="min-w-0">
                    <div className={`text-sm ${dim}`}>{t.text}</div>
                    <div className={`text-xs ${dim}`}>{g.title} · {t.owner === 'me' ? 'You' : 'Unassigned'}{t.removedBy ? ` · removed by ${t.removedBy === 'ai' ? 'AI' : 'you'}` : ''}</div>
                  </div>
                  <Button variant="outline" size="sm" onClick={() => void act('tasks_set_review', { id: t.id, review: 'open' }, t.id)}>Restore</Button>
                </div>
              )))}
              <p className={`text-xs ${dim}`}>Removed tasks are never sent. They are deleted with their meeting&apos;s recording.</p>
            </>
          )}
        </div>
      </div>
      {tab === 'review' && loaded && <SendBar summary={summary} sending={sending} result={result} onSend={() => void send(summary.sendableIds)} onView={() => setTab('sent')} />}
    </>
  );
}
