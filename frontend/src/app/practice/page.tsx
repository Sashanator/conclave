'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { SetupList } from '@/components/Practice/SetupList';
import { SessionList } from '@/components/Practice/SessionList';
import { SetupEditor } from '@/components/Practice/SetupEditor';
import { ReportView } from '@/components/Practice/ReportView';
import { CallView } from '@/components/Practice/CallView';
import {
  blockedReason, characterName, planText, type PracticePrepPayload, type PracticeStatus, type SessionDetail, type SessionSummary, type SetupSummary, type VoicesProgressPayload,
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
  // Each launch takes a number. Cancel or a newer launch moves it on, so a stale start's result is ignored.
  const attempt = useRef(0);

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
      return s;
    } catch (err) {
      toast.error('Could not load Practice', { description: String(err) });
      return null;
    }
  }, []);

  // A meeting or an import can end while the list shows, so the start gate is re-read on focus and every 5 s
  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await invoke<PracticeStatus>('practice_status'));
    } catch {
      // Keeps the last status; the next focus or tick tries again
    }
  }, []);
  const onList = view.kind === 'list';
  useEffect(() => {
    if (!onList) return;
    const timer = window.setInterval(refreshStatus, 5000);
    window.addEventListener('focus', refreshStatus);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener('focus', refreshStatus);
    };
  }, [onList, refreshStatus]);

  // A session still running when the page opens goes back to its call view (R22)
  useEffect(() => {
    refresh().then(async (s) => {
      if (!s?.running_session) return;
      const sessionId = s.running_session;
      try {
        const detail = await invoke<SessionDetail>('practice_get_session', { id: sessionId });
        setView({ kind: 'call', sessionId, setupName: detail.summary.setup_name, character: detail.character });
      } catch (err) {
        toast.error('Could not open the running session', { description: String(err) });
      }
    });
  }, [refresh]);

  const backToList = () => {
    setView({ kind: 'list' });
    refresh();
  };

  // The plan step runs inside practice_start; the overlay follows its prep events until it resolves
  const launch = async (setup: SetupSummary, skipPlan: boolean) => {
    const id = ++attempt.current;
    stage.current = !skipPlan && setup.materials.length > 0 ? 'plan' : 'loading';
    setOverlay({ kind: 'start', setup, stage: stage.current, error: null });
    const stop = await listen<PracticePrepPayload>('practice-prep', (e) => {
      if (attempt.current !== id) return;
      stage.current = e.payload.stage;
      setOverlay((o) => (o?.kind === 'start' ? { ...o, stage: e.payload.stage, error: e.payload.error } : o));
    });
    // Cancel may have landed while the listener registered; then nothing starts
    if (attempt.current !== id) {
      stop();
      return;
    }
    try {
      const sessionId = await invoke<string>('practice_start', { setupId: setup.id, skipPlan });
      if (attempt.current !== id) return;
      setOverlay(null);
      setView({ kind: 'call', sessionId, setupName: setup.name, character: setup.character });
    } catch (err) {
      if (attempt.current !== id) return;
      // Read through a cast: TypeScript keeps the narrowing from the assignment above across the await
      const current = stage.current as PracticePrepPayload['stage'] | null;
      if (current === 'plan') {
        // The plan failed. Its prep event may arrive after this rejection, so the overlay is set here too
        stage.current = 'failed';
        setOverlay({ kind: 'start', setup, stage: 'failed', error: String(err) });
      } else if (current !== 'failed') {
        setOverlay(null);
        toast.error('Could not start the session', { description: String(err) });
      }
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
    attempt.current++;
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
  if (view.kind === 'report') return <ReportView sessionId={view.sessionId} onBack={backToList} />;

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
