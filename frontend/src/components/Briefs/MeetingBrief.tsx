'use client';

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { BriefView } from '@/components/Briefs/BriefView';

type State = { isCall: boolean; briefId: string | null };

/** The brief of a 📞 meeting, with a Regenerate button; nothing for other meetings. */
export function MeetingBrief({ meetingId }: { meetingId: string }) {
  const [state, setState] = useState<State | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      setState(await invoke<State>('brief_for_meeting', { meetingId }));
    } catch (error) {
      toast.error(`Failed to load the brief: ${String(error)}`);
    }
  }, [meetingId]);

  useEffect(() => {
    setState(null);
    load();
  }, [load]);

  const regenerate = async () => {
    setBusy(true);
    try {
      await invoke('brief_regenerate', { meetingId });
      await load();
    } catch (error) {
      // The old brief stays on screen.
      toast.error(String(error));
    } finally {
      setBusy(false);
    }
  };

  if (!state?.isCall) return null;
  return (
    <section aria-label="Brief" className="space-y-3 p-3">
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-base font-semibold">Brief</h3>
        <Button variant="outline" size="sm" disabled={busy} onClick={regenerate}>
          Regenerate brief
        </Button>
      </div>
      {state.briefId ? (
        <BriefView key={state.briefId} id={state.briefId} />
      ) : (
        <p className="text-sm text-muted-foreground">No brief yet</p>
      )}
    </section>
  );
}
