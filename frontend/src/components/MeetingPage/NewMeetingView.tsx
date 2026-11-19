'use client';

import { useEffect, useRef, useState } from 'react';
import Link from 'next/link';
import { toast } from 'sonner';
import { prepForMeeting, savePrep, type MeetingPrep } from '@/lib/prep';
import type { MeetingRow } from '@/lib/meetings';
import { PrepTickList } from '@/components/Prep/PrepTickList';
import { PrepChat } from '@/components/Prep/PrepChat';
import { Button } from '@/components/ui/button';
import { LanguagesSpoken } from '@/components/LanguagesSpoken';
import { useConfig } from '@/contexts/ConfigContext';
import { AudioSummaryLine } from './AudioSummaryLine';

/** A meeting with no recording yet: Start on top, the audio line and languages, with the prep notes in a second column. */
export function NewMeetingView({ row, live, disabled, onStart, header }: {
  row: MeetingRow;
  /** The name and category block, rendered at the top of the first column. */
  header: React.ReactNode;
  /** Another meeting is being recorded: its title replaces the button. */
  live: { id: string; title: string } | null;
  /** A recording is starting, stopping or saving. */
  disabled: boolean;
  onStart: () => void;
}) {
  const { transcriptModelConfig: { provider } } = useConfig(); // Parakeet only auto-detects: no spoken-languages list, as in Settings
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

  // Saves run one after another against the latest saved text, so the typing pause, the blur and the unmount never double-save.
  const prepRef = useRef<MeetingPrep | null>(null);
  prepRef.current = prep;
  const chain = useRef<Promise<unknown>>(Promise.resolve());
  const doSave = async (text: string): Promise<MeetingPrep | null> => {
    const cur = prepRef.current;
    if (!loaded) return null;
    if (text === saved.current && cur?.id) return cur;
    try {
      const next = await savePrep({ meetingId: row.id, eventUid: cur?.eventUid ?? null, eventStart: cur?.eventStart ?? null, eventTitle: row.title, material: text, templateId: row.templateId, items: cur?.items ?? [] });
      saved.current = text;
      prepRef.current = next;
      setPrep(next);
      return next;
    } catch (e) { toast.error(`Failed to save Prep: ${String(e)}`); return null; }
  };
  const saveMaterial = (): Promise<MeetingPrep | null> => {
    const run = chain.current.then(() => doSave(material));
    chain.current = run;
    return run;
  };
  const saveRef = useRef(saveMaterial);
  saveRef.current = saveMaterial;
  // A tray or detector start leaves this page with no blur: save while typing, and once more on unmount.
  useEffect(() => {
    if (!loaded || material === saved.current) return;
    const t = setTimeout(() => { void saveRef.current(); }, 800);
    return () => clearTimeout(t);
  }, [material, loaded]);
  const materialRef = useRef(material);
  materialRef.current = material;
  // Nothing typed (or already saved): leaving must not insert an empty prep row.
  useEffect(() => () => { if (materialRef.current !== saved.current) void saveRef.current(); }, []);

  const start = live ? (
    <p className="text-sm text-muted-foreground">
      Recording in {live.title} · <Link className="text-primary hover:underline" href={`/meeting-details?id=${encodeURIComponent(live.id)}`}>Open</Link>
    </p>
  ) : (
    <div className="space-y-1">
      <Button type="button" size="lg" onClick={onStart} disabled={disabled} className="h-12 gap-2 px-8 text-base font-semibold">
        <span aria-hidden className="h-2.5 w-2.5 rounded-full" style={{ backgroundColor: 'hsl(var(--rec))' }} />
        Start recording
      </Button>
      <p className="text-xs text-muted-foreground">Recording goes into this meeting.</p>
    </div>
  );

  return (
    <div className="@container">
      <div className="grid gap-10 @[80rem]:grid-cols-2">
        <div className="min-w-0 max-w-[880px] space-y-6">
          {header}
          {start}
          <AudioSummaryLine changeable={!disabled} />
          {provider !== 'parakeet' && <LanguagesSpoken />}
        </div>
        <section aria-labelledby="prep-notes" className="min-w-0 max-w-[880px] space-y-2">
          <h2 id="prep-notes" className="text-base font-semibold text-foreground">Prep material and chat</h2>
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
      </div>
    </div>
  );
}
