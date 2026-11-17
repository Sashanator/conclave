'use client';

import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import {
  approvalLabel, approvePrep, charCount, countLabel, materialLimit, savePrep, sendPrep, SUGGEST_MESSAGE,
  type MeetingPrep,
} from '@/lib/prep';

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);
const fieldClass = 'w-full rounded-md border border-input bg-background px-3 py-2 text-sm';

/**
 * Prep material, AI chat, Suggest and Approve for one meeting, under a disclosure.
 * `save` is given when the parent edits the material itself (the New-state notes): the textarea is then hidden.
 */
export function PrepChat({ meetingId, title, templateId, prep, onChange, save }: {
  meetingId: string;
  title: string;
  templateId: string | null;
  prep: MeetingPrep | null;
  onChange(p: MeetingPrep): void;
  save?: () => Promise<MeetingPrep | null>;
}) {
  const [open, setOpen] = useState(false);
  const [material, setMaterial] = useState(prep?.material ?? '');
  const [draft, setDraft] = useState('');
  const [limit, setLimit] = useState<number | null>(null);
  const [pending, setPending] = useState(false);
  const busy = useRef(false);
  useEffect(() => {
    let active = true;
    materialLimit().then((n) => { if (active) setLimit(n); }).catch((e) => toast.error(errorText(e)));
    return () => { active = false; };
  }, []);

  const count = charCount(save ? (prep?.material ?? '') : material);
  const overLimit = limit !== null && count > limit;

  // Saves the material when it changed or no prep exists yet; nothing is deleted or replaced by an unchanged save.
  const persist = async (): Promise<MeetingPrep | null> => {
    if (save) return save();
    if (prep?.id && prep.material === material) return prep;
    try {
      const next = await savePrep({
        meetingId, eventUid: prep?.eventUid ?? null, eventStart: prep?.eventStart ?? null,
        eventTitle: prep?.eventTitle ?? title, material, templateId: prep?.templateId ?? templateId, items: prep?.items ?? [],
      });
      onChange(next);
      return next;
    } catch (e) { toast.error(errorText(e)); return null; }
  };
  const run = async (action: (saved: MeetingPrep) => Promise<void>) => {
    if (busy.current || overLimit) return;
    busy.current = true; setPending(true);
    try { const saved = await persist(); if (saved) await action(saved); }
    finally { busy.current = false; setPending(false); }
  };
  const send = (message: string) => message.trim() ? run(async (saved) => {
    try {
      const reply = await sendPrep(saved.id, message);
      onChange(reply.prep);
      setDraft((d) => d === message ? '' : d);
    } catch (e) { toast.error(errorText(e)); }
  }) : Promise.resolve();

  const chat = prep?.chat ?? [];
  const hasMaterial = (save ? (prep?.material ?? '') : material).trim() !== '';
  return (
    <section aria-label="Prep material and chat" className="space-y-3 border-t border-border p-3">
      <button type="button" aria-expanded={open} onClick={() => setOpen(!open)} className="text-sm font-medium">
        Prep material and chat
      </button>
      {open && <div className="space-y-3">
        {!save && <>
          <textarea aria-label="Material" disabled={pending} className={`${fieldClass} min-h-[120px] resize-y`} value={material}
            onChange={(e) => setMaterial(e.target.value)} aria-invalid={overLimit} />
          {limit !== null && <p className={`text-xs ${overLimit ? 'text-destructive' : 'text-muted-foreground'}`}>{countLabel(count, limit)}</p>}
        </>}
        <div aria-label="Prep chat" aria-live="polite" className="space-y-2">
          {hasMaterial && chat.length === 0 && <Button variant="outline" size="sm" disabled={pending || overLimit} onClick={() => send(SUGGEST_MESSAGE)}>Suggest</Button>}
          {chat.map((m, i) => (
            <article key={i} className="rounded-md border border-border p-3 text-sm">
              <p className="font-medium">{m.role === 'user' ? 'You' : 'Assistant'}</p>
              <p className="whitespace-pre-wrap break-words">{m.text}</p>
              {m.role === 'assistant' && m.listNotUpdated && <p className="text-xs text-muted-foreground">list not updated</p>}
            </article>
          ))}
          {pending && <p role="status" className="text-sm text-muted-foreground">Working…</p>}
        </div>
        <textarea aria-label="Message" disabled={pending} className={`${fieldClass} min-h-[60px] resize-none`} value={draft} placeholder="Ask about your prep…"
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send(draft); } }} />
        <div className="flex flex-wrap items-center gap-2">
          <Button disabled={pending || overLimit || !draft.trim()} onClick={() => send(draft)}>Send</Button>
          <Button variant="outline" disabled={pending || overLimit} onClick={() => run(async (saved) => {
            try { onChange(await approvePrep(saved.id)); } catch (e) { toast.error(errorText(e)); }
          })}>Approve</Button>
          {prep && approvalLabel(prep) && <span className="text-xs text-muted-foreground">{approvalLabel(prep)}</span>}
        </div>
      </div>}
    </section>
  );
}
