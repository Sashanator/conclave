'use client';

import { useEffect, useState, type ReactNode } from 'react';
import { toast } from 'sonner';
import { prepForMeeting, type MeetingPrep } from '@/lib/prep';
import { PrepTickList } from '@/components/Prep/PrepTickList';
import { PrepChat } from '@/components/Prep/PrepChat';

/** The meeting's prep list and, collapsed, its material and AI chat; `briefSlot` is where the brief goes. */
export function PrepTab({ meetingId, briefSlot = null }: { meetingId: string; briefSlot?: ReactNode }) {
  const [prep, setPrep] = useState<MeetingPrep | null>(null);
  // The chat waits for the load: saving over a prep that failed to load would replace it.
  const [loaded, setLoaded] = useState(false);
  useEffect(() => {
    let active = true;
    setPrep(null); setLoaded(false);
    prepForMeeting(meetingId)
      .then((value) => { if (active) { setPrep(value); setLoaded(true); } })
      .catch((error) => { if (active) toast.error(`Failed to load Prep: ${String(error)}`); });
    return () => { active = false; };
  }, [meetingId]);
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto">
      {briefSlot}
      {prep ? <PrepTickList prep={prep} onChange={setPrep} /> : <p className="p-3 text-sm text-muted-foreground">No prep for this meeting.</p>}
      {loaded && <PrepChat key={meetingId} meetingId={meetingId} title="" templateId={null} prep={prep} onChange={setPrep} />}
    </div>
  );
}
