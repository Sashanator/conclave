'use client';
import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useCopilot, type CopilotCard } from '@/contexts/CopilotContext';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { actionLabel, answerToNote, MAX_QUESTION_CHARS, type CopilotAction } from '@/lib/copilot';
import { formatRecordingTime } from '@/lib/meeting-notes';

const ACTIONS: CopilotAction[] = ['catch_up', 'what_to_ask', 'explain'];

export function CopilotPane({ locked }: { locked: boolean }) {
  const { transcripts } = useTranscripts();
  const { isRecording } = useRecordingState();
  const { cards, pending, ask } = useCopilot();
  const [draft, setDraft] = useState('');
  const hasFinal = transcripts.some((t) => !t.is_partial);
  const blocked = pending || locked || !hasFinal;
  const hint = !isRecording ? 'Copilot works during a recording' : !hasFinal ? 'Copilot starts once there is transcript' : null;

  const submit = () => {
    const question = draft.trim();
    if (!question || blocked) return;
    ask('ask', question);
    setDraft('');
  };

  return (
    <section aria-label="Copilot" className="flex min-h-0 min-w-0 flex-1 flex-col bg-background">
      <div className="flex flex-wrap gap-1.5 border-b border-border p-2">
        {ACTIONS.map((action) => (
          <Button key={action} variant="outline" size="sm" disabled={blocked} onClick={() => ask(action)}>
            {actionLabel(action)}
          </Button>
        ))}
      </div>
      {hint && <p className="px-3 pt-2 text-xs text-muted-foreground">{hint}</p>}
      <div className="flex-1 space-y-2 overflow-y-auto p-3">
        {cards.map((card) => <CopilotCardView key={card.id} card={card} locked={locked} />)}
      </div>
      <div className="border-t border-border p-2 pb-24">
        <textarea
          aria-label="Ask Copilot"
          disabled={blocked}
          maxLength={MAX_QUESTION_CHARS}
          placeholder="Ask about this meeting…"
          className="w-full resize-none rounded-md border border-input bg-background px-3 py-2 text-sm min-h-[60px] disabled:opacity-60"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); submit(); }
          }}
        />
      </div>
    </section>
  );
}

function CopilotCardView({ card, locked }: { card: CopilotCard; locked: boolean }) {
  const router = useRouter();
  const { addNote } = useTranscripts();
  const { activeDuration } = useRecordingState();
  const { pending, cancel, retry, markAdded } = useCopilot();
  const range = card.range ? `${formatRecordingTime(card.range[0])}–${formatRecordingTime(card.range[1])}` : null;
  const answer = card.answer ?? '';

  return (
    <article aria-label={card.label} className="rounded-md border border-border p-3 text-sm">
      <div className="flex items-start justify-between gap-2">
        <h4 className="line-clamp-2 font-medium">{card.label}</h4>
        <span className="whitespace-nowrap text-xs text-muted-foreground">
          {formatRecordingTime(card.askedAt)}{range && ` ${range}`}
        </span>
      </div>

      {card.status === 'pending' && (
        <div className="mt-2 flex items-center justify-between">
          <span role="status" aria-label="Loading"><Loader2 className="h-4 w-4 animate-spin text-muted-foreground" aria-hidden="true" /></span>
          <Button variant="ghost" size="sm" onClick={() => cancel()}>Cancel</Button>
        </div>
      )}

      {card.status === 'done' && (
        <>
          {/* Text children only: the answer is model output and must never be parsed as markup. */}
          <div className="mt-2 whitespace-pre-wrap break-words">{answer}</div>
          <div className="mt-2 flex gap-2">
            <Button variant="outline" size="sm" disabled={locked || card.added}
              onClick={() => { addNote(answerToNote(card.label, answer), activeDuration ?? card.askedAt); markAdded(card.id); }}>
              {card.added ? 'Added' : 'Add to notes'}
            </Button>
            <Button variant="ghost" size="sm" onClick={() => { void navigator.clipboard.writeText(answer); }}>Copy</Button>
          </div>
        </>
      )}

      {card.status === 'error' && (
        <>
          <p className="mt-2 whitespace-pre-wrap break-words text-destructive">{card.error}</p>
          <div className="mt-2 flex gap-2">
            <Button variant="outline" size="sm" disabled={pending || locked} onClick={() => retry(card.id)}>Retry</Button>
            <Button variant="ghost" size="sm" onClick={() => router.push('/settings?tab=copilot')}>Copilot settings</Button>
          </div>
        </>
      )}
    </article>
  );
}
