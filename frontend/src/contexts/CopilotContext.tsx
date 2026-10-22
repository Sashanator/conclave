'use client';

import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { actionLabel, buildCopilotRequest, resolveAnswerLanguage, type CopilotAction, type CopilotSettings } from '@/lib/copilot';
import { readPinnedSummaryLanguageDefault } from '@/lib/summary-language-preferences';

export const LIVE_SIDE_TAB_KEY = 'conclave.liveSideTab'; // 'notes' | 'copilot'
export type LiveSideTab = 'notes' | 'copilot';

export function readLiveSideTab(): LiveSideTab {
  try { return localStorage.getItem(LIVE_SIDE_TAB_KEY) === 'copilot' ? 'copilot' : 'notes'; } catch { return 'notes'; }
}

export function writeLiveSideTab(tab: LiveSideTab): void {
  try { localStorage.setItem(LIVE_SIDE_TAB_KEY, tab); } catch { /* optional */ }
}

export interface CopilotCard {
  id: number;
  action: CopilotAction;
  label: string;
  /** Recording time of the ask, in seconds. */
  askedAt: number;
  range: [number, number] | null;
  status: 'pending' | 'done' | 'error';
  answer?: string;
  error?: string;
  question?: string;
  added?: boolean;
}

interface CopilotContextType {
  cards: CopilotCard[];
  pending: boolean;
  ask(action: CopilotAction, question?: string): void;
  cancel(): void;
  retry(id: number): void;
  markAdded(id: number): void;
}

/** The one call in flight. Cancel and stop clear it, so a late answer finds it gone and is ignored. */
interface InFlight { cardId: number; requestId: number }

const CopilotContext = createContext<CopilotContextType | null>(null);

export function CopilotProvider({ children }: { children: ReactNode }) {
  const { transcriptsRef, notesRef, currentMeetingId } = useTranscripts();
  const { isRecording, isStopping, activeDuration } = useRecordingState();
  const [cards, setCards] = useState<CopilotCard[]>([]);
  // Seeded from the clock so ids from before a webview reload never repeat (the Rust slot guard keys on them).
  const seqRef = useRef(Date.now());
  const inFlightRef = useRef<InFlight | null>(null);

  // A stopping recording takes its pending call and its cards with it. isStopping comes first: the backend keeps isRecording true through the stop tail.
  useEffect(() => {
    if (isRecording && !isStopping) return;
    const f = inFlightRef.current;
    inFlightRef.current = null;
    if (f) invoke('copilot_cancel', { requestId: f.requestId }).catch(() => {});
    setCards([]);
  }, [isRecording, isStopping]);

  const run = (cardId: number, action: CopilotAction, question: string | undefined, askedAt: number) => {
    const f: InFlight = { cardId, requestId: ++seqRef.current };
    inFlightRef.current = f;
    setCards((prev) => prev.map((c) => (c.id === cardId ? { ...c, askedAt, status: 'pending', answer: undefined, error: undefined } : c)));
    const finish = (patch: Partial<CopilotCard>) => {
      if (inFlightRef.current !== f) return;
      inFlightRef.current = null;
      setCards((prev) => prev.map((c) => (c.id === cardId ? { ...c, ...patch } : c)));
    };
    void (async () => {
      try {
        const settings = await invoke<CopilotSettings>('copilot_get_settings');
        if (inFlightRef.current !== f) return;
        const language = resolveAnswerLanguage(settings.answer_language, readPinnedSummaryLanguageDefault());
        const { request, range } = buildCopilotRequest(action, transcriptsRef.current, notesRef.current, language, question, currentMeetingId);
        setCards((prev) => prev.map((c) => (c.id === cardId ? { ...c, range } : c)));
        const answer = await invoke<string>('copilot_ask', { requestId: f.requestId, request });
        finish({ status: 'done', answer });
      } catch (err) {
        const error = String(err);
        // Only a cancel rejects with "Cancelled", and cancel has already removed the card.
        if (error === 'Cancelled') {
          if (inFlightRef.current === f) {
            inFlightRef.current = null;
            setCards((prev) => prev.filter((c) => c.id !== cardId));
          }
          return;
        }
        finish({ status: 'error', error });
      }
    })();
  };

  const ask = (action: CopilotAction, question?: string) => {
    if (inFlightRef.current) return;
    const id = ++seqRef.current;
    const askedAt = activeDuration ?? 0;
    setCards((prev) => [{ id, action, label: actionLabel(action, question), askedAt, range: null, status: 'pending', question }, ...prev]);
    run(id, action, question, askedAt);
  };

  const cancel = () => {
    const f = inFlightRef.current;
    if (!f) return;
    inFlightRef.current = null;
    invoke('copilot_cancel', { requestId: f.requestId }).catch(() => {});
    setCards((prev) => prev.filter((c) => c.id !== f.cardId));
  };

  const retry = (id: number) => {
    const card = cards.find((c) => c.id === id);
    if (!card || inFlightRef.current) return;
    run(id, card.action, card.question, activeDuration ?? 0);
  };

  const markAdded = (id: number) => setCards((prev) => prev.map((c) => (c.id === id ? { ...c, added: true } : c)));

  const value: CopilotContextType = {
    cards,
    pending: cards.some((c) => c.status === 'pending'),
    ask,
    cancel,
    retry,
    markAdded,
  };
  return <CopilotContext.Provider value={value}>{children}</CopilotContext.Provider>;
}

export function useCopilot(): CopilotContextType {
  const context = useContext(CopilotContext);
  if (!context) {
    throw new Error('useCopilot must be used within a CopilotProvider');
  }
  return context;
}
