'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { SetupList } from '@/components/Practice/SetupList';
import { SessionList } from '@/components/Practice/SessionList';
import { SetupEditor } from '@/components/Practice/SetupEditor';
import { CallView } from '@/components/Practice/CallView';
import {
  blockedReason, characterName, planText, type PracticePrepPayload, type PracticeStatus, type SessionSummary, type SetupSummary, type VoicesProgressPayload,
} from '@/lib/practice';

type View =
  | { kind: 'list' }
  | { kind: 'edit'; id?: string }
  | { kind: 'call'; sessionId: string; setupName: string; character: string }
  | { kind: 'report'; sessionId: string };

type Overlay =
  | { kind: 'voices'; percent: number }
  | { kind: 'start'; setup: SetupSummary; stage: PracticePrepPayload['stage']; error: string | null };

export default function PracticePage() {
  const [view, setView] = useState<View>({ kind: 'list' });
  const [status, setStatus] = useState<PracticeStatus | null>(null);
  const [setups, setSetups] = useState<SetupSummary[]>([]);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [overlay, setOverlay] = useState<Overlay | null>(null);
  // The prep stage the backend last reported, read when start rejects
  const stage = useRef<PracticePrepPayload['stage'] | null>(null);
  // Set by Cancel, so the start that rejects afterwards closes quietly
  const cancelled = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const [s, list, past] = await Promise.all([
        invoke<PracticeStatus>('practice_status'),
        invoke<SetupSummary[]>('practice_list_setups'),
        invoke<SessionSummary[]>('practice_list_sessions'),
      ]);
      setStatus(s);
      setSetups(list);
      setSessions(past);
    } catch (err) {
      toast.error('Could not load Practice', { description: String(err) });
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const backToList = () => {
    setView({ kind: 'list' });
    refresh();
  };

  // The plan step runs inside practice_start; the overlay follows its prep events until it resolves
  const launch = async (setup: SetupSummary, skipPlan: boolean) => {
    cancelled.current = false;
    stage.current = null;
    setOverlay({ kind: 'start', setup, stage: !skipPlan && setup.materials.length > 0 ? 'plan' : 'loading', error: null });
    const stop = await listen<PracticePrepPayload>('practice-prep', (e) => {
      stage.current = e.payload.stage;
      setOverlay((o) => (o?.kind === 'start' ? { ...o, stage: e.payload.stage, error: e.payload.error } : o));
    });
    try {
      const sessionId = await invoke<string>('practice_start', { setupId: setup.id, skipPlan });
      setOverlay(null);
      setView({ kind: 'call', sessionId, setupName: setup.name, character: setup.character });
    } catch (err) {
      // A failed plan keeps the overlay open with Retry and Start without questions
      if (cancelled.current || stage.current === 'failed') return;
      setOverlay(null);
      toast.error('Could not start the session', { description: String(err) });
    } finally {
      stop();
    }
  };

  const start = async (setup: SetupSummary) => {
    if (status && !status.voices_ready) {
      setOverlay({ kind: 'voices', percent: 0 });
      const stop = await listen<VoicesProgressPayload>('practice-voices-progress', (e) => {
        setOverlay({ kind: 'voices', percent: e.payload.percent });
      });
      try {
        await invoke('practice_download_voices');
        setStatus((s) => (s ? { ...s, voices_ready: true } : s));
      } catch (err) {
        setOverlay(null);
        toast.error('Could not download the voices', { description: String(err) });
        return;
      } finally {
        stop();
      }
    }
    await launch(setup, false);
  };

  const cancelStart = () => {
    cancelled.current = true;
    setOverlay(null);
    invoke('practice_cancel_start').catch(() => {});
  };

  const deleteSession = async (session: SessionSummary) => {
    if (!window.confirm(`Delete the past session "${session.setup_name}" and its report?`)) return;
    try {
      await invoke('practice_delete_session', { id: session.id });
    } catch (err) {
      toast.error('Could not delete the session', { description: String(err) });
    }
    refresh();
  };

  if (view.kind === 'call') {
    return (
      <CallView
        sessionId={view.sessionId}
        setupName={view.setupName}
        character={view.character}
        onEnded={() => setView({ kind: 'report', sessionId: view.sessionId })}
      />
    );
  }
  // Placeholder: the report view comes in a later task
  if (view.kind === 'report') return <div className="p-8 text-muted-foreground">Report view</div>;

  const editSetup = view.kind === 'edit' && view.id ? setups.find((s) => s.id === view.id) : undefined;
  if (view.kind === 'edit' && (!view.id || editSetup)) {
    return (
      <div className="h-full overflow-y-auto">
        <div className="max-w-3xl mx-auto p-8">
          <SetupEditor key={view.id ?? 'new'} setup={editSetup ?? null} onDone={backToList} onChanged={refresh} />
        </div>
      </div>
    );
  }

  const reason = status ? blockedReason(status) : null;
  const canStart = status !== null && reason === null && overlay === null;

  return (
    <div className="h-full overflow-y-auto">
      <div className="max-w-6xl mx-auto p-8 space-y-6">
        <h1 className="text-3xl font-bold">Practice</h1>
        {reason && <p className="rounded-lg border border-border bg-card p-4 text-sm text-muted-foreground">{reason}</p>}

        <div className="grid gap-4 lg:grid-cols-2 items-start">
          <SetupList
            setups={setups}
            canStart={canStart}
            onNew={() => setView({ kind: 'edit' })}
            onEdit={(s) => setView({ kind: 'edit', id: s.id })}
            onStart={start}
          />
          <SessionList
            sessions={sessions}
            onOpen={(s) => setView({ kind: 'report', sessionId: s.id })}
            onDelete={deleteSession}
          />
        </div>
      </div>

      {overlay && (
        <div role="dialog" aria-modal="true" className="fixed inset-0 z-50 flex items-center justify-center bg-background/80">
          <div className="w-96 space-y-4 rounded-lg border border-border bg-card p-6 shadow-lg">
            {overlay.kind === 'voices' && <p>{`Downloading voices… ${overlay.percent}%`}</p>}
            {overlay.kind === 'start' && overlay.stage === 'plan' && <p>{planText(characterName(overlay.setup.character))}</p>}
            {overlay.kind === 'start' && overlay.stage === 'loading' && <p>Loading…</p>}
            {overlay.kind === 'start' && overlay.stage === 'failed' && (
              <p className="text-sm text-destructive">{overlay.error}</p>
            )}
            <div className="flex justify-end gap-2">
              {overlay.kind === 'start' && overlay.stage === 'failed' && (
                <>
                  <Button variant="outline" size="sm" onClick={() => launch(overlay.setup, true)}>Start without questions</Button>
                  <Button size="sm" onClick={() => launch(overlay.setup, false)}>Retry</Button>
                </>
              )}
              {overlay.kind === 'start' && <Button variant="outline" size="sm" onClick={cancelStart}>Cancel</Button>}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
