'use client';

import { useEffect, useState, type ReactNode } from 'react';
import { toast } from 'sonner';
import { prepForMeeting, type MeetingPrep } from '@/lib/prep';
import { PrepTickList } from '@/components/Prep/PrepTickList';

/** The meeting's prep list; `briefSlot` is where the brief goes (Task 49). */
export function PrepTab({ meetingId, briefSlot = null }: { meetingId: string; briefSlot?: ReactNode }) {
  const [prep, setPrep] = useState<MeetingPrep | null>(null);
  useEffect(() => {
    let active = true;
    setPrep(null);
    prepForMeeting(meetingId)
      .then((value) => { if (active) setPrep(value); })
      .catch((error) => { if (active) toast.error(`Failed to load Prep: ${String(error)}`); });
    return () => { active = false; };
  }, [meetingId]);
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto">
      {briefSlot}
      {prep ? <PrepTickList prep={prep} onChange={setPrep} /> : <p className="p-3 text-sm text-muted-foreground">No prep for this meeting.</p>}
    </div>
  );
}
