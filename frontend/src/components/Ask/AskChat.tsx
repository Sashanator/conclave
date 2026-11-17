'use client';

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useRouter } from 'next/navigation';
import { Button } from '@/components/ui/button';
import { citationTarget, citeLabel, nextAskRequestId, splitAnswer, type AskAnswer, type Turn } from '@/lib/ask';
import type { AskScope } from '@/lib/ai-sidebar';
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
export function AskChat({ scope, allowAll }: { scope: AskScope; allowAll: boolean }) {
  const router = useRouter();
  const [turns, setTurns] = useState<Chat[]>([]);
  const [draft, setDraft] = useState('');
  const [all, setAll] = useState(false);
  const [menu, setMenu] = useState(false);
  const meetingIds = allowAll && all ? null : scope.meetingIds;
  const chip = meetingIds === null ? 'All meetings' : `${scope.label} · ${meetingIds.length} ${meetingIds.length === 1 ? 'meeting' : 'meetings'}`;
  const inFlight = useRef<number | null>(null);
  const busy = turns.some((t) => t.status === 'pending');

  const cancelInFlight = () => {
    if (inFlight.current !== null) invoke('ask_cancel', { requestId: inFlight.current }).catch((e) => console.error('Could not cancel the question:', e));
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
    invoke<AskAnswer>('ask_meetings', { requestId, question, history, meetingIds })
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
      <div className="relative flex items-center justify-between gap-2 border-b border-border bg-background px-4 py-3">
        <h2 className="text-base font-semibold">Ask</h2>
        {allowAll ? (
          <div className="relative min-w-0 flex-1">
            <button
              type="button"
              aria-haspopup="menu"
              aria-expanded={menu}
              onClick={() => setMenu((m) => !m)}
              className="max-w-full truncate rounded-full border border-border px-2.5 py-1 text-xs hover:bg-accent"
            >
              {chip}
            </button>
            {menu && (
              <div role="menu" className="absolute left-0 top-full z-10 mt-1 w-48 rounded-md border border-border bg-popover p-1 shadow-md">
                {[false, true].map((a) => (
                  <button
                    key={String(a)}
                    type="button"
                    role="menuitemradio"
                    aria-checked={all === a}
                    onClick={() => { setAll(a); setMenu(false); }}
                    className="block w-full rounded px-2 py-1.5 text-left text-xs hover:bg-accent"
                  >
                    {a ? 'All meetings' : `${scope.label} · ${scope.meetingIds?.length ?? 0}`}
                  </button>
                ))}
              </div>
            )}
          </div>
        ) : (
          <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">{chip}</span>
        )}
        <Button variant="outline" size="sm" onClick={newChat}>
          New chat
        </Button>
      </div>

      <div role="log" aria-live="polite" className="flex-1 space-y-4 overflow-y-auto p-4">
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
