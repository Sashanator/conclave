'use client';

import { useEffect, useRef, useState, type MutableRefObject } from 'react';
import { useSearchParams } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { setUnsavedChanges } from '@/lib/unsaved-guard';
import {
  approvalLabel, approvePrep, charCount, countLabel, deletePrep, getPrep, groupByDay,
  materialLimit, prepOverview, savePrep, sendPrep, SUGGEST_MESSAGE,
  type MeetingPrep, type PrepOverview, type PrepItem,
} from '@/lib/prep';

type Event = { uid: string; start: string; title: string; templateId: string | null; unreadableId?: number };
type Templates = { templates: Array<{ id: string; name: string }>; errors: string[] };
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);
const fieldClass = 'w-full rounded-md border border-input bg-background px-3 py-2 text-sm';

function attachSaved(local: PrepItem[], sent: PrepItem[], saved: PrepItem[], sameList: boolean): PrepItem[] {
  return local.map(item => {
    const index = sent.indexOf(item);
    const stored = index < 0 ? undefined : saved[index];
    if (!stored) return item;
    return sameList ? stored : item.id === '' ? { ...item, id: stored.id } : item;
  });
}

export function PrepPage() {
  const params = useSearchParams();
  const uid = params.get('event');
  const start = params.get('start');
  const [overview, setOverview] = useState<PrepOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Event | null>(null);
  const guard = useRef<(() => Promise<boolean>) | null>(null);
  const selectionRequest = useRef(0);

  const refresh = async () => {
    try { setOverview(await prepOverview()); setError(null); }
    catch (err) { setError(errorText(err)); }
  };
  useEffect(() => {
    let live = true;
    prepOverview().then(value => { if (live) setOverview(value); })
      .catch(err => { if (live) setError(errorText(err)); });
    return () => { live = false; };
  }, []);

  const select = async (event: Event) => {
    const request = ++selectionRequest.current;
    if (guard.current && !await guard.current()) return;
    if (request === selectionRequest.current) setSelected(event);
  };
  useEffect(() => {
    if (!overview || !uid || !start) return;
    const upcoming = overview.upcoming.find(e => e.uid === uid && e.start === start);
    const other = overview.other.find(e => e.eventUid === uid && e.eventStart === start);
    const event = upcoming ?? (other ? { uid, start, title: other.eventTitle, templateId: null, unreadableId: other.reason === 'unreadable' ? other.id : undefined } : { uid, start, title: '', templateId: null });
    void select(event);
    // Follow URL changes, without reopening the editor when the badges refresh.
  }, [uid, start, overview !== null]);

  return (
    <div className="flex h-full min-h-0 flex-col bg-muted/50">
      <h1 className="px-6 py-4 text-3xl font-bold">Prep</h1>
      <div className="grid min-h-0 flex-1 grid-cols-1 overflow-y-auto lg:grid-cols-[minmax(200px,1fr)_minmax(300px,2fr)_minmax(240px,1fr)]">
        <aside aria-label="Calendar events" className="space-y-4 overflow-y-auto border-r border-border p-4">
          {error && <div role="alert">{error}<Button variant="outline" onClick={refresh}>Retry</Button></div>}
          {!overview && !error && <p role="status">Loading calendar…</p>}
          {overview && <>
            {!overview.connected && <p>Calendar is not connected. <a className="underline" href="/settings?tab=knowledge">Connect in Settings → Knowledge</a></p>}
            {groupByDay(overview.upcoming).map(group => <section key={group.day}>
              <h2 className="mb-2 text-sm font-semibold">{group.day}</h2>
              <ul className="divide-y divide-border rounded-lg border border-border bg-card">
                {group.events.map(event => <li key={`${event.uid}:${event.start}`}>
                  <button type="button" className="w-full space-y-1 px-3 py-2 text-left hover:bg-accent" aria-pressed={selected?.uid === event.uid && selected.start === event.start} onClick={() => select(event)}>
                    <span className="mr-2 text-xs text-muted-foreground">{new Date(event.start).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</span>
                    <span>{event.title}</span>
                    <span className="block text-xs text-muted-foreground">{event.templateName ?? 'General'} {event.prep && <span className="rounded border px-1">{event.prep.approved ? 'Approved' : 'draft'}</span>}</span>
                  </button>
                </li>)}
              </ul>
            </section>)}
            {overview.upcoming.length === 0 && <p className="text-sm text-muted-foreground">No upcoming events.</p>}
            <section>
              <h2 className="mb-2 text-sm font-semibold">Past and orphaned preps</h2>
              {overview.other.length === 0 && <p className="text-sm text-muted-foreground">No past or orphaned preps.</p>}
              {overview.other.map(prep => <button type="button" key={prep.id} className="block w-full rounded-md border border-border bg-card p-3 text-left hover:bg-accent" onClick={() => select({ uid: prep.eventUid, start: prep.eventStart, title: prep.eventTitle, templateId: null, unreadableId: prep.reason === 'unreadable' ? prep.id : undefined })}>
                <span>{prep.eventTitle}</span><span className="ml-2 text-xs">{prep.approved ? 'Approved' : 'draft'}</span>
                <span className="block text-xs text-muted-foreground">{new Date(prep.eventStart).toLocaleString()}</span>
                {prep.reason === 'unreadable' && <span className="block text-xs text-muted-foreground">data is unreadable</span>}
                {prep.reason === 'not_in_calendar' && <span className="block text-xs text-muted-foreground">event no longer in calendar</span>}
              </button>)}
            </section>
          </>}
        </aside>
        {selected?.unreadableId !== undefined ? <UnreadablePrep key={selected.unreadableId} id={selected.unreadableId} onDeleted={() => { setSelected(null); void refresh(); }} /> : selected ? <PrepEditor key={`${selected.uid}:${selected.start}`} event={selected} guard={guard} onUpdated={refresh} onDeleted={() => { guard.current = null; setSelected(null); void refresh(); }} /> : <p className="p-6 text-sm text-muted-foreground">Select an event to prepare.</p>}
      </div>
    </div>
  );
}

function UnreadablePrep({ id, onDeleted }: { id: number; onDeleted(): void }) {
  const [pending, setPending] = useState(false);
  return <section className="space-y-3 p-4">
    <p>data is unreadable</p>
    <Button variant="destructive" disabled={pending} onClick={async () => {
      if (pending || !window.confirm('Delete this prep?')) return;
      setPending(true);
      try { await deletePrep(id); onDeleted(); }
      catch (err) { toast.error(errorText(err)); }
      finally { setPending(false); }
    }}>Delete prep</Button>
  </section>;
}

function PrepEditor({ event, guard, onUpdated, onDeleted }: {
  event: Event;
  guard: MutableRefObject<(() => Promise<boolean>) | null>;
  onUpdated: () => Promise<void>;
  onDeleted: () => void;
}) {
  const [prep, setPrep] = useState<MeetingPrep | null>(null);
  const current = useRef<MeetingPrep | null>(null);
  const revision = useRef(0);
  const savedRevision = useRef(0);
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const busy = useRef(false);
  const [pending, setPending] = useState(false);
  const [limit, setLimit] = useState<number | null>(null);
  const [templates, setTemplates] = useState<Templates['templates']>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loadAttempt, setLoadAttempt] = useState(0);
  const [draft, setDraft] = useState('');
  const [sendError, setSendError] = useState<{ text: string; message: string } | null>(null);
  const live = useRef(true);
  const syncDirty = () => { if (live.current) setUnsavedChanges(savedRevision.current !== revision.current); };
  useEffect(() => () => setUnsavedChanges(false), []);

  const replace = (value: MeetingPrep) => { current.current = value; if (live.current) setPrep(value); };
  useEffect(() => {
    live.current = true;
    const load = async () => {
      setLoadError(null);
      try {
        const [existing, cap, listing] = await Promise.all([getPrep(event.uid, event.start), materialLimit(), invoke<Templates>('api_list_templates').catch(err => {
          toast.error(errorText(err));
          return { templates: [], errors: [] };
        })]);
        if (!live.current) return;
        replace(existing ?? {
          id: 0, eventUid: event.uid, eventStart: event.start, eventTitle: event.title,
          templateId: event.templateId, material: '', chat: [], items: [], approvedAt: null, createdAt: '', updatedAt: '',
        });
        setLimit(cap); setTemplates(listing.templates);
        listing.errors.forEach(error => toast.error(error));
      } catch (err) { if (live.current) setLoadError(errorText(err)); }
    };
    void load();
    return () => { live.current = false; };
  }, [event.uid, event.start, event.title, event.templateId, loadAttempt]);

  const persist = (): Promise<MeetingPrep | null> => {
    const task = queue.current.then(async () => {
      const value = current.current;
      if (!value || limit === null) return null;
      if (charCount(value.material) > limit) { toast.error(countLabel(charCount(value.material), limit)); return null; }
      if (savedRevision.current === revision.current && value.id) return value;
      const version = revision.current;
      const sentItems = value.items.filter(item => item.text.trim() !== '');
      try {
        const saved = await savePrep({ eventUid: value.eventUid, eventStart: value.eventStart, eventTitle: value.eventTitle, material: value.material, templateId: value.templateId, items: sentItems });
        savedRevision.current = version;
        // A write completing must never replace edits made while it was in flight.
        if (version === revision.current) replace({ ...saved, items: attachSaved(current.current!.items, sentItems, saved.items, true) });
        else if (current.current) {
          const local = current.current;
          const items = attachSaved(local.items, sentItems, saved.items, false);
          replace({ ...local, items, id: saved.id, approvedAt: saved.approvedAt, updatedAt: saved.updatedAt });
        }
        void onUpdated();
        return saved;
      } catch (err) { toast.error(errorText(err)); return null; }
      finally { syncDirty(); }
    });
    queue.current = task;
    return task;
  };
  const commit = () => { if (savedRevision.current !== revision.current) void persist(); };
  useEffect(() => {
    const check = async () => {
      if (busy.current) { toast.error('Wait for the current action to finish, then switch.'); return false; }
      busy.current = true; setPending(true);
      try {
        await queue.current;
        return savedRevision.current === revision.current || Boolean(await persist());
      } finally {
        busy.current = false;
        if (live.current) setPending(false);
      }
    };
    guard.current = check;
    return () => { if (guard.current === check) guard.current = null; };
  });

  const edit = (patch: Partial<MeetingPrep>, save = false) => {
    if (!current.current || busy.current) return;
    revision.current += 1;
    syncDirty();
    replace({ ...current.current, ...patch });
    if (save) void persist();
  };
  const run = async (action: () => Promise<void>) => {
    if (busy.current) return;
    busy.current = true; setPending(true);
    try { await action(); }
    finally { busy.current = false; if (live.current) setPending(false); }
  };
  const send = async (message: string) => {
    if (!message.trim()) return;
    await run(async () => {
      setSendError(null);
      const saved = await persist();
      if (!saved) { setSendError({ text: 'Could not save prep. Your message has been kept.', message }); return; }
      try {
        const reply = await sendPrep(saved.id, message);
        replace(reply.prep); setDraft(d => d === message ? '' : d);
        void onUpdated();
      } catch (err) { setSendError({ text: errorText(err), message }); }
    });
  };
  const updateItem = (index: number, patch: Partial<PrepItem>) => {
    if (!current.current) return;
    edit({ items: current.current.items.map((item, i) => i === index ? { ...item, ...patch, byUser: true } : item) });
  };

  if (loadError) return <div role="alert" className="p-4">{loadError}<Button variant="outline" onClick={() => setLoadAttempt(attempt => attempt + 1)}>Retry</Button></div>;
  if (!prep || limit === null) return <p role="status" className="p-4">Loading prep…</p>;
  const count = charCount(prep.material);
  const overLimit = count > limit;
  return <>
    <section aria-label="Prep material and chat" className="flex min-h-0 flex-col border-r border-border bg-background">
      <div className="space-y-3 border-b border-border p-4">
        <h2 className="font-semibold">{prep.eventTitle}</h2>
        <label className="block text-sm" htmlFor="prep-material">Material</label>
        <textarea id="prep-material" aria-label="Material" disabled={pending} className={`${fieldClass} min-h-[160px] resize-y`} value={prep.material} onChange={e => edit({ material: e.target.value })} onBlur={commit} aria-invalid={overLimit} aria-describedby="prep-count" />
        <p id="prep-count" className={`text-xs ${overLimit ? 'text-destructive' : 'text-muted-foreground'}`}>{countLabel(count, limit)}</p>
        <label className="block text-sm">Add .txt file
          <input aria-label="Add .txt file" className="mt-1 block w-full text-sm" type="file" accept=".txt,text/plain" disabled={pending} onChange={async e => {
            const input = e.target;
            const file = input.files?.[0];
            input.value = '';
            if (!file) return;
            if (file.size > limit * 4) { toast.error('The file is too large to add'); return; }
            await run(async () => {
              let bytes: ArrayBuffer;
              try { bytes = await file.arrayBuffer(); }
              catch (err) { toast.error(errorText(err)); return; }
              let text: string;
              try { text = new TextDecoder('utf-8', { fatal: true }).decode(bytes); }
              catch { toast.error('The file is not UTF-8 text'); return; }
              if (!live.current || !current.current) return;
              const next = current.current.material ? `${current.current.material}\n\n${text}` : text;
              if (charCount(next) > limit) { toast.error(countLabel(charCount(next), limit)); return; }
              revision.current += 1;
              syncDirty();
              replace({ ...current.current, material: next });
              await persist();
            });
          }} />
        </label>
        <label className="block text-sm">Template
          <select aria-label="Template" className={`${fieldClass} mt-1`} disabled={pending} value={templates.some(t => t.id === prep.templateId) ? (prep.templateId ?? '') : ''} onChange={e => edit({ templateId: e.target.value || null }, true)}>
            <option value="">Auto</option>
            {templates.map(template => <option key={template.id} value={template.id}>{template.name}</option>)}
          </select>
        </label>
      </div>
      <div className="flex-1 space-y-2 overflow-y-auto p-3" aria-label="Prep chat" aria-live="polite">
        {prep.material.trim() && prep.chat.length === 0 && <Button variant="outline" size="sm" disabled={pending || overLimit} onClick={() => send(SUGGEST_MESSAGE)}>Suggest</Button>}
        {prep.chat.map((message, i) => <article key={i} className="rounded-md border border-border p-3 text-sm">
          <p className="font-medium">{message.role === 'user' ? 'You' : 'Assistant'}</p>
          <p className="whitespace-pre-wrap break-words">{message.text}</p>
          {message.role === 'assistant' && message.listNotUpdated && <p className="text-xs text-muted-foreground">list not updated</p>}
        </article>)}
        {sendError && <article role="alert" className="rounded-md border border-border p-3 text-sm">
          <p className="whitespace-pre-wrap break-words text-destructive">{sendError.text}</p>
          <Button variant="outline" size="sm" disabled={pending || overLimit} onClick={() => send(sendError.message)}>Retry</Button>
        </article>}
        {pending && <p role="status" className="text-sm text-muted-foreground">Working…</p>}
      </div>
      <div className="space-y-2 border-t border-border p-3">
        <textarea aria-label="Message" className={`${fieldClass} min-h-[60px] resize-none`} disabled={pending} value={draft} placeholder="Ask about your prep…" onChange={e => setDraft(e.target.value)} onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); if (!overLimit) void send(draft); } }} />
        <Button disabled={pending || overLimit || !draft.trim()} onClick={() => send(draft)}>Send</Button>
      </div>
    </section>
    <section aria-label="Prep checklist" className="space-y-3 overflow-y-auto bg-background p-4">
      <h2 className="font-semibold">Checklist</h2>
      {prep.items.length === 0 && <p className="text-sm text-muted-foreground">No items yet.</p>}
      {prep.items.map((item, i) => <div key={i} className="space-y-2 rounded-md border border-border p-3">
        <input aria-label={`Item ${i + 1}`} className={fieldClass} disabled={pending} value={item.text} onChange={e => updateItem(i, { text: e.target.value })} onBlur={commit} onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); commit(); } }} />
        <span className="rounded bg-muted px-2 py-1 text-xs">{item.kind}</span>
        <Button aria-label={`Remove item ${i + 1}`} variant="ghost" size="sm" disabled={pending} onClick={() => edit({ items: prep.items.filter((_, index) => index !== i) }, true)}>Remove</Button>
      </div>)}
      <Button variant="outline" disabled={pending} onClick={() => edit({ items: [...prep.items, { id: '', text: '', kind: 'question', ticked: false, byUser: true }] })}>Add item</Button>
      <div className="space-y-2 border-t border-border pt-3">
        <Button disabled={pending || overLimit} onClick={() => run(async () => {
          const saved = await persist();
          if (!saved) return;
          try { replace(await approvePrep(saved.id)); void onUpdated(); }
          catch (err) { toast.error(errorText(err)); }
        })}>Approve</Button>
        {approvalLabel(prep) && <p className="text-xs text-muted-foreground">{approvalLabel(prep)}</p>}
      </div>
      <Button variant="destructive" disabled={pending || !prep.id} onClick={() => {
        if (!window.confirm('Delete this prep?')) return;
        return run(async () => {
          await queue.current;
          try { await deletePrep(current.current!.id); onDeleted(); }
          catch (err) { toast.error(errorText(err)); }
        });
      }}>Delete prep</Button>
    </section>
  </>;
}
