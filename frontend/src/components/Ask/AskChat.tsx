'use client';

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useRouter } from 'next/navigation';
import { Button } from '@/components/ui/button';
import { citationTarget, citeLabel, nextAskRequestId, splitAnswer, type AskAnswer, type Turn } from '@/lib/ask';
import { meetingHref } from '@/lib/meeting-target';

// The backend keeps this many history entries too; sending more would be dropped there.
const HISTORY_TURNS = 6;

type Chat = {
  requestId: number;
  question: string;
  status: 'pending' | 'answered' | 'failed';
  answer?: AskAnswer;
  error?: string;
};

// In-memory chat: the question and answer of each turn. Answers are shown as text, never markup.
export function AskChat() {
  const router = useRouter();
  const [turns, setTurns] = useState<Chat[]>([]);
  const [draft, setDraft] = useState('');
  const inFlight = useRef<number | null>(null);
  const busy = turns.some((t) => t.status === 'pending');

  const cancelInFlight = () => {
    if (inFlight.current !== null) void invoke('ask_cancel', { requestId: inFlight.current });
    inFlight.current = null;
  };

  // Leaving the page cancels the question still running
  useEffect(() => () => cancelInFlight(), []);

  const settle = (requestId: number, patch: Partial<Chat>) => {
    if (inFlight.current === requestId) inFlight.current = null;
    setTurns((ts) => ts.map((t) => (t.requestId === requestId && t.status === 'pending' ? { ...t, ...patch } : t)));
  };

  // `prior` is the chat before this question; only its answered turns are history
  const ask = (requestId: number, question: string, prior: Chat[]) => {
    inFlight.current = requestId;
    const history: Turn[] = prior
      .filter((t) => t.status === 'answered')
      .flatMap((t) => [
        { role: 'user' as const, text: t.question },
        { role: 'assistant' as const, text: t.answer!.text },
      ])
      .slice(-HISTORY_TURNS);
    invoke<AskAnswer>('ask_meetings', { requestId, question, history })
      .then((answer) => settle(requestId, { status: 'answered', answer }))
      .catch((err) => settle(requestId, { status: 'failed', error: String(err) }));
  };

  const send = () => {
    const question = draft.trim();
    if (!question || busy) return;
    const requestId = nextAskRequestId();
    setTurns((ts) => [...ts, { requestId, question, status: 'pending' }]);
    setDraft('');
    ask(requestId, question, turns);
  };

  const retry = (index: number) => {
    const requestId = nextAskRequestId();
    const { question } = turns[index];
    setTurns((ts) => ts.map((t, i) => (i === index ? { requestId, question, status: 'pending' } : t)));
    ask(requestId, question, turns.slice(0, index));
  };

  const cancel = (requestId: number) => {
    cancelInFlight();
    setTurns((ts) => ts.map((t) => (t.requestId === requestId && t.status === 'pending' ? { ...t, status: 'failed', error: 'Cancelled' } : t)));
  };

  const newChat = () => {
    cancelInFlight();
    setTurns([]);
  };

  return (
    <div className="flex h-full flex-col bg-muted/50">
      <div className="flex items-center justify-between border-b border-border bg-background px-8 py-4">
        <h1 className="text-2xl font-bold">Ask</h1>
        <Button variant="outline" size="sm" onClick={newChat}>
          New chat
        </Button>
      </div>

      <div role="log" aria-live="polite" className="flex-1 space-y-6 overflow-y-auto p-8">
        {turns.map((t, index) => (
          <div key={t.requestId} className="space-y-2">
            <p className="ml-auto max-w-[80%] whitespace-pre-wrap rounded-lg bg-primary/10 p-3">{t.question}</p>

            {t.status === 'pending' && (
              <div className="flex items-center gap-3 text-sm text-muted-foreground">
                <span>Searching your meetings…</span>
                <Button variant="outline" size="sm" onClick={() => cancel(t.requestId)}>
                  Cancel
                </Button>
              </div>
            )}

            {t.status === 'failed' && (
              <div className="flex items-center gap-3 text-sm text-red-600">
                <span>{t.error}</span>
                <Button variant="outline" size="sm" disabled={busy} onClick={() => retry(index)}>
                  Retry
                </Button>
              </div>
            )}

            {t.answer && (
              <div className="whitespace-pre-wrap rounded-lg bg-background p-3 leading-relaxed">
                {splitAnswer(t.answer.text, t.answer.citations).map((part, i) =>
                  part.kind === 'text' ? (
                    <span key={i}>{part.text}</span>
                  ) : (
                    <a
                      key={i}
                      href={meetingHref(citationTarget(part.citation))}
                      onClick={(e) => {
                        e.preventDefault();
                        router.push(meetingHref(citationTarget(part.citation)));
                      }}
                      className="mx-0.5 rounded bg-yellow-50 px-1 text-xs text-yellow-700 hover:bg-yellow-100 dark:bg-yellow-950/40 dark:text-yellow-300"
                    >
                      {citeLabel(part.citation)}
                    </a>
                  ),
                )}
              </div>
            )}
          </div>
        ))}
      </div>

      <div className="border-t border-border bg-background p-4">
        <textarea
          rows={2}
          value={draft}
          disabled={busy}
          aria-label="Ask about your meetings"
          placeholder="Ask about your meetings…"
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              send();
            }
          }}
          className="w-full resize-none rounded-md border border-border bg-background p-3 text-sm outline-none focus:ring-2 focus:ring-ring disabled:opacity-60"
        />
      </div>
    </div>
  );
}
