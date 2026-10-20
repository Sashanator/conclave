import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import {
  characterName, formatClock, NOTICE_TEXT, phaseText,
  type Phase, type PracticeStatePayload, type PracticeTurnPayload,
} from '@/lib/practice';

// Space typed into a text field is text, not a tap
const TYPING = 'input, textarea, [contenteditable]:not([contenteditable="false"])';
// No tap while the character's reply is not yet playing, or the call has ended
const NO_TAP: Phase[] = ['Loading', 'Transcribing', 'Ended'];

export function CallView({ sessionId, setupName, character, onEnded }: {
  sessionId: string;
  setupName: string;
  character: string;
  onEnded: () => void;
}) {
  const [state, setState] = useState<PracticeStatePayload>({
    session_id: sessionId, phase: 'Loading', character, elapsed_secs: 0, caption: null, notice: null,
  });
  const [turns, setTurns] = useState<PracticeTurnPayload[]>([]);
  const [showTranscript, setShowTranscript] = useState(false);
  const name = characterName(state.character);
  const canTap = !NO_TAP.includes(state.phase);

  useEffect(() => {
    let active = true;
    const unlisten: Array<() => void> = [];
    Promise.all([
      listen<PracticeStatePayload>('practice-state', (e) => {
        if (e.payload.session_id !== sessionId) return;
        // A notice stays until the phase changes
        setState((prev) => ({
          ...e.payload,
          notice: e.payload.notice ?? (prev.phase === e.payload.phase ? prev.notice : null),
        }));
      }),
      listen<PracticeTurnPayload>('practice-turn', (e) => {
        if (e.payload.session_id !== sessionId) return;
        setTurns((prev) => [...prev.filter((t) => t.idx !== e.payload.idx), e.payload].sort((a, b) => a.idx - b.idx));
      }),
    ]).then((fns) => {
      if (active) unlisten.push(...fns);
      else fns.forEach((f) => f());
    });
    return () => {
      active = false;
      unlisten.forEach((f) => f());
    };
  }, [sessionId]);

  useEffect(() => {
    if (state.phase === 'Ended') onEnded();
  }, [state.phase]);

  const tap = () => {
    invoke('practice_tap').catch((err) => toast.error('Could not register your turn', { description: String(err) }));
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== ' ' || e.repeat || !canTap) return;
      // Ctrl, Alt or Meta with Space is a shortcut for something else
      if (e.ctrlKey || e.altKey || e.metaKey) return;
      if ((e.target as Element).closest?.(TYPING)) return;
      e.preventDefault();
      tap();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [canTap]);

  const end = () => {
    invoke('practice_end').catch((err) => toast.error('Could not end the session', { description: String(err) }));
  };

  return (
    <div className="h-full overflow-y-auto">
      <div className="max-w-3xl mx-auto p-8 flex flex-col items-center min-h-[380px]">
        <div className="flex w-full items-center justify-between text-sm text-muted-foreground">
          <span>{`${setupName} · ${formatClock(state.elapsed_secs)}`}</span>
          <Button variant="outline" size="sm" onClick={end}>End session → report</Button>
        </div>

        <div className="mt-8 mb-2 flex h-24 w-24 items-center justify-center rounded-full border-[3px] border-primary bg-muted text-4xl">
          {name.split(/\s+/).at(-1)?.charAt(0)}
        </div>
        <div className="text-base font-semibold">{name}</div>
        <p className={`text-sm ${state.phase === 'CharacterSpeaking' ? 'text-primary' : 'text-muted-foreground'}`}>
          {phaseText(state.phase, name)}
        </p>

        {state.caption && (
          <div className="mt-4 w-full max-w-xl rounded-lg border border-border bg-card p-4 text-center">{state.caption}</div>
        )}

        <Button size="lg" className="mt-6 h-14 px-10 text-base" disabled={!canTap} onClick={tap}>
          ● Tap to talk <span className="opacity-70">Space</span>
        </Button>
        <div role="status" className="mt-3 h-5 text-sm text-muted-foreground">
          {state.notice ? NOTICE_TEXT[state.notice] : null}
        </div>

        <Button variant="ghost" size="sm" className="mt-2" onClick={() => setShowTranscript((v) => !v)}>
          {showTranscript ? 'Hide full transcript ▴' : 'Show full transcript ▾'}
        </Button>
        {showTranscript && (
          <ul className="mt-2 w-full max-w-2xl space-y-2 text-sm">
            {turns.map((t) => (
              <li key={t.idx} className="flex gap-2">
                <span className="shrink-0 font-medium text-muted-foreground">{t.speaker === 'user' ? 'You' : name}</span>
                <span>{t.text}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
