'use client';

import { useEffect, useRef, useState } from 'react';
import Link from 'next/link';
import { toast } from 'sonner';
import { prepForMeeting, savePrep, type MeetingPrep } from '@/lib/prep';
import type { MeetingRow } from '@/lib/meetings';
import { PrepTickList } from '@/components/Prep/PrepTickList';
import { PrepChat } from '@/components/Prep/PrepChat';
import { AudioLine } from './AudioLine';

/** A meeting with no recording yet: prep notes, the Audio line and Start recording. */
export function NewMeetingView({ row, live, disabled, onStart }: {
  row: MeetingRow;
  /** Another meeting is being recorded: its title replaces the button. */
  live: { id: string; title: string } | null;
  /** A recording is starting, stopping or saving. */
  disabled: boolean;
  onStart: () => void;
}) {
  const [prep, setPrep] = useState<MeetingPrep | null>(null);
  const [material, setMaterial] = useState('');
  const saved = useRef('');
  // The notes stay read-only until the load answered: saving over a prep that failed to load would replace it.
  const [loaded, setLoaded] = useState(false);
  useEffect(() => {
    let active = true;
    setPrep(null); setMaterial(''); saved.current = ''; setLoaded(false);
    prepForMeeting(row.id)
      .then((p) => { if (!active) return; if (p) { setPrep(p); setMaterial(p.material); saved.current = p.material; } setLoaded(true); })
      .catch((e) => { if (active) toast.error(`Failed to load Prep: ${String(e)}`); });
    return () => { active = false; };
  }, [row.id]);

  const saveMaterial = async (): Promise<MeetingPrep | null> => {
    if (!loaded) return null;
    if (material === saved.current && prep?.id) return prep;
    try {
      const next = await savePrep({ meetingId: row.id, eventUid: prep?.eventUid ?? null, eventStart: prep?.eventStart ?? null, eventTitle: row.title, material, templateId: row.templateId, items: prep?.items ?? [] });
      saved.current = material;
      setPrep(next);
      return next;
    } catch (e) { toast.error(`Failed to save Prep: ${String(e)}`); return null; }
  };

  return (
    <div className="mx-auto w-full max-w-3xl space-y-6 p-6">
      <section aria-labelledby="prep-notes" className="space-y-2">
        <h2 id="prep-notes" className="text-sm font-medium text-foreground">Prep notes</h2>
        <textarea
          aria-labelledby="prep-notes"
          value={material}
          onChange={(e) => setMaterial(e.target.value)}
          onBlur={() => material !== saved.current ? saveMaterial() : undefined}
          readOnly={!loaded}
          rows={6}
          placeholder="Agenda, questions, context for this meeting"
          className="w-full rounded-md border border-border bg-background p-3 text-sm"
        />
        {prep && prep.items.length > 0 && <PrepTickList prep={prep} onChange={setPrep} />}
        {loaded && <PrepChat meetingId={row.id} title={row.title} templateId={row.templateId} prep={prep} onChange={setPrep} save={saveMaterial} />}
      </section>
      <AudioLine disabled={false} />
      {live ? (
        <p className="text-sm text-muted-foreground">
          Recording in {live.title} · <Link className="text-primary hover:underline" href={`/meeting-details?id=${encodeURIComponent(live.id)}`}>Open</Link>
        </p>
      ) : (
        <div className="space-y-1">
          <button
            type="button"
            onClick={onStart}
            disabled={disabled}
            className="rounded-md bg-primary px-8 py-3 text-base font-semibold text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
          >
            Start recording
          </button>
          <p className="text-xs text-muted-foreground">Recording goes into this meeting.</p>
        </div>
      )}
    </div>
  );
}
