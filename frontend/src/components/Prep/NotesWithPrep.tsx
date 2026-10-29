'use client';

import { useEffect, useState, type ReactNode } from 'react';
import { toast } from 'sonner';
import { prepCounter, prepForMeeting, type MeetingPrep } from '@/lib/prep';
import { PrepTickList } from './PrepTickList';

export function NotesWithPrep({ meetingId, notes }: { meetingId: string; notes: ReactNode }) {
  const [prep, setPrep] = useState<MeetingPrep | null>(null);
  const [tab, setTab] = useState<'notes' | 'prep'>('notes');

  useEffect(() => {
    let active = true;
    setPrep(null);
    prepForMeeting(meetingId)
      .then((value) => { if (active) setPrep(value); })
      .catch((error) => { if (active) toast.error(`Failed to load Prep: ${String(error)}`); });
    return () => { active = false; };
  }, [meetingId]);

  // One tree shape with or without a prep: the notes child keeps its place and never remounts when the prep arrives.
  const showPrep = prep !== null && tab === 'prep';

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background">
      {prep && (
        <div role="tablist" aria-label="Notes and Prep" className="flex gap-1 border-b border-border px-2">
          {(['notes', 'prep'] as const).map((name) => (
            <button
              key={name}
              type="button"
              role="tab"
              aria-selected={tab === name}
              onClick={() => setTab(name)}
              className={`-mb-px border-b-2 px-2 py-2 text-sm ${tab === name ? 'border-primary font-medium text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground'}`}
            >
              {name === 'notes' ? 'My notes' : prepCounter(prep.items)}
            </button>
          ))}
        </div>
      )}
      <div className={showPrep ? 'hidden' : 'flex min-h-0 min-w-0 flex-1 flex-col'}>{notes}</div>
      {prep && (
        <div className={showPrep ? 'flex min-h-0 min-w-0 flex-1 flex-col' : 'hidden'}>
          <PrepTickList prep={prep} onChange={setPrep} />
        </div>
      )}
    </div>
  );
}
