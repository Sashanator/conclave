'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { continueFailed, type ContinueMeeting, type ContinueStatus } from '@/lib/resume';

/** Live view: offers to continue an earlier meeting, or says which one the recording joins. */
export function ContinueControl() {
  const [status, setStatus] = useState<ContinueStatus>({ offer: null, target: null });

  useEffect(() => {
    invoke<ContinueStatus>('continue_status').then(setStatus).catch(() => {});
    const changed = listen<ContinueMeeting>('continue-target-changed', (e) => setStatus((s) => ({ ...s, target: e.payload })));
    return () => {
      changed.then((fn) => fn());
    };
  }, []);

  if (status.target) {
    return <div className="absolute left-0 right-0 top-0 z-10 px-4 pt-2 text-center text-xs text-muted-foreground">{`Continuing "${status.target.title}"`}</div>;
  }
  if (!status.offer) return null;
  const offer = status.offer;
  return (
    <div className="absolute left-0 right-0 top-0 z-10 flex justify-center pt-2">
      <button
        type="button"
        className="max-w-[80%] truncate rounded-md border border-border bg-card px-3 py-1 text-xs hover:bg-accent"
        onClick={() => {
          invoke<ContinueMeeting>('continue_set_target', { meetingId: offer.meeting_id })
            .then((m) => setStatus((s) => ({ ...s, target: m })))
            .catch(continueFailed);
        }}
      >
        {`Continue "${offer.title}"`}
      </button>
    </div>
  );
}
