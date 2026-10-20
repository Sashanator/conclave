'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import {
  characterName, formatClock, minutesLabel, NOTICE_TEXT, YOU_WAV_RATE,
  type ItemKind, type PronHint, type ReportItem, type ReportStatusPayload, type SessionDetail, type TurnRow,
} from '@/lib/practice';

const SECTIONS: { kind: ItemKind; title: string }[] = [
  { kind: 'Grammar', title: 'Grammar' },
  { kind: 'Unnatural', title: 'Sounds unnatural' },
  { kind: 'Clarity', title: 'Could be clearer' },
];

/** The words both versions share at the start and end are kept; the middle is the change. */
function changedParts(quote: string, better: string): { head: string; changed: string; tail: string } {
  const a = quote.split(/\s+/).filter(Boolean);
  const b = better.split(/\s+/).filter(Boolean);
  let head = 0;
  while (head < a.length && head < b.length && a[head] === b[head]) head++;
  let tail = 0;
  while (tail < a.length - head && tail < b.length - head && a[a.length - 1 - tail] === b[b.length - 1 - tail]) tail++;
  return {
    head: a.slice(0, head).join(' '),
    changed: a.slice(head, a.length - tail).join(' '),
    tail: a.slice(a.length - tail).join(' '),
  };
}

function Quote({ item }: { item: ReportItem }) {
  if (item.kind !== 'Grammar') return <>{item.quote}</>;
  const { head, changed, tail } = changedParts(item.quote, item.better);
  return (
    <>
      {head && `${head} `}
      {changed && <span className="line-through text-destructive">{changed}</span>}
      {changed && tail && ' '}
      {tail}
    </>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div className="flex-1 rounded-lg border border-border bg-card p-4 text-center">
      <div className="text-2xl font-bold tabular-nums">{`${value}`}</div>
      <div className="text-sm text-muted-foreground">{label}</div>
    </div>
  );
}

export function ReportView({ sessionId, onBack }: { sessionId: string; onBack: () => void }) {
  const [detail, setDetail] = useState<SessionDetail | null>(null);
  const [tab, setTab] = useState<'report' | 'transcript'>('report');
  const audioRef = useRef<HTMLAudioElement>(null);
  // Where the clip being played ends, in seconds; the audio pauses there
  const stopAt = useRef<number | null>(null);

  const load = useCallback(async () => {
    try {
      setDetail(await invoke<SessionDetail>('practice_get_session', { id: sessionId }));
    } catch (err) {
      toast.error('Could not load the session', { description: String(err) });
    }
  }, [sessionId]);

  useEffect(() => {
    let gone = false;
    let stop: (() => void) | null = null;
    // The listener is up before the first load, so a report change that lands meanwhile is not missed
    void listen<ReportStatusPayload>('practice-report', (e) => {
      if (e.payload.session_id === sessionId) void load();
    }).then((off) => {
      if (gone) off();
      else stop = off;
    }).finally(() => {
      if (!gone) void load();
    });
    return () => {
      gone = true;
      stop?.();
    };
  }, [sessionId, load]);

  // Retry and Generate report share one call; the ref drops a second click before the disabled state renders
  const [busy, setBusy] = useState(false);
  const inFlight = useRef(false);
  const retry = async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    try {
      await invoke('practice_retry_report', { sessionId });
    } catch (err) {
      toast.error('Could not retry the report', { description: String(err) });
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
    void load();
  };

  // from and to are seconds into you.wav
  const play = (from: number, to: number) => {
    const el = audioRef.current;
    if (!el) return;
    stopAt.current = to;
    el.currentTime = from;
    void el.play().catch((e) => toast.error('Could not play the recording', { description: String(e) }));
  };

  const pauseAtClipEnd = () => {
    const el = audioRef.current;
    if (el && stopAt.current !== null && el.currentTime >= stopAt.current) {
      el.pause();
      stopAt.current = null;
    }
  };

  // ▶ for the learner's turn with this index, when its clip exists
  const playButton = (d: SessionDetail, turnIdx: number) => {
    const turn: TurnRow | undefined = d.turns.find((t) => t.idx === turnIdx);
    if (!turn || !d.audio_path || turn.wav_offset === null || turn.wav_len === null) return null;
    const from = turn.wav_offset / YOU_WAV_RATE;
    const to = (turn.wav_offset + turn.wav_len) / YOU_WAV_RATE;
    return (
      <button type="button" className="rounded px-1.5 py-0.5 hover:bg-accent tabular-nums" onClick={() => play(from, to)}>
        {`▶ ${formatClock(turn.at_secs)}`}
      </button>
    );
  };

  // ▶ you for a pronunciation hint: its span within the turn's clip
  const hintPlayButton = (d: SessionDetail, hint: PronHint) => {
    const turn = d.turns.find((t) => t.idx === hint.turn);
    if (!turn || !d.audio_path || turn.wav_offset === null) return null;
    const turnStart = turn.wav_offset / YOU_WAV_RATE;
    return (
      <button type="button" className="rounded px-1.5 py-0.5 hover:bg-accent" onClick={() => play(turnStart + hint.start_secs, turnStart + hint.end_secs)}>
        ▶ you
      </button>
    );
  };

  const speak = async (word: string) => {
    try {
      await invoke('practice_speak_word', { sessionId, word });
    } catch (err) {
      toast.error('Could not say the word', { description: String(err) });
    }
  };

  if (!detail) {
    return (
      <div className="h-full overflow-y-auto">
        <div className="max-w-4xl mx-auto p-8 space-y-6">
          <Button variant="ghost" size="sm" onClick={onBack}>← Practice</Button>
          <p className="text-muted-foreground">Loading…</p>
        </div>
      </div>
    );
  }

  const s = detail.summary;
  const name = characterName(detail.character);
  const day = new Date(s.started_at).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  const length = minutesLabel(s.duration_secs);
  const report = detail.report;
  const fillers = Object.entries(report?.fluency.fillers ?? {}).filter(([, n]) => n > 0).sort(([, a], [, b]) => b - a);
  const generate = (label: string) => (
    <Button size="sm" disabled={busy} onClick={retry}>{label}</Button>
  );
  // Patterns from the same setup first; the order within each group is the one the server sent
  const recurring = detail.recurring;
  const patterns = [...(recurring?.patterns ?? [])].sort((a, b) => Number(b.same_setup) - Number(a.same_setup));
  const trend = recurring?.filler_trend ?? [];
  const showRecurring = patterns.length > 0 || trend.length >= 2;

  return (
    <div className="h-full overflow-y-auto">
      <div className="max-w-4xl mx-auto p-8 space-y-6">
        <Button variant="ghost" size="sm" onClick={onBack}>← Practice</Button>
        <audio ref={audioRef} src={detail.audio_path ? convertFileSrc(detail.audio_path) : undefined} preload="auto" onTimeUpdate={pauseAtClipEnd} />

        <div className="space-y-2">
          <h1 className="text-3xl font-bold">{`${s.setup_name} · ${day} · ${length}`}</h1>
          {s.end_reason === 'meeting' && <p className="text-sm text-muted-foreground">{NOTICE_TEXT.MeetingStarted}</p>}
          {s.end_reason === 'interrupted' && <p className="text-sm text-muted-foreground">This session was interrupted (Conclave closed)</p>}
        </div>

        <div className="flex gap-2 border-b border-border">
          {(['report', 'transcript'] as const).map((t) => (
            <button
              key={t}
              type="button"
              aria-pressed={tab === t}
              onClick={() => setTab(t)}
              className={`-mb-px border-b-2 px-3 py-2 text-sm ${tab === t ? 'border-primary font-medium text-foreground' : 'border-transparent text-muted-foreground'}`}
            >
              {t === 'report' ? 'Report' : 'Transcript'}
            </button>
          ))}
        </div>

        {tab === 'transcript' ? (
          <ul className="space-y-3">
            {detail.turns.map((t) => (
              <li key={t.idx} className="flex gap-3">
                <span className="w-14 shrink-0 tabular-nums text-sm text-muted-foreground">{formatClock(t.at_secs)}</span>
                <div className="min-w-0">
                  <div className="text-sm font-medium">{t.speaker === 'user' ? 'You' : name}</div>
                  <p>{t.speaker === 'user' ? (t.verbatim_text ?? t.live_text) : t.live_text}</p>
                </div>
              </li>
            ))}
          </ul>
        ) : report ? (
          <>
            <div className="flex gap-3">
              <Stat label="fillers" value={report.fluency.filler_total} />
              <Stat label="words/min" value={Math.round(report.fluency.wpm)} />
              <Stat label="long pauses" value={report.fluency.long_pauses} />
            </div>

            {SECTIONS.map(({ kind, title }) => {
              const items = report.items.filter((i) => i.kind === kind);
              return (
                <section key={kind} className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-3">
                  <h3 className="text-lg font-semibold text-foreground">{`${title} (${items.length})`}</h3>
                  {items.map((item, i) => (
                    <div key={i} className="space-y-1">
                      <p>&ldquo;<Quote item={item} />&rdquo;</p>
                      <p className="text-emerald-500">{item.better}</p>
                      <p className="text-sm text-muted-foreground flex flex-wrap items-center gap-2">
                        <span>{item.note}</span>
                        {playButton(detail, item.turn)}
                        {item.echoed && <span className="rounded px-1.5 py-0.5 text-xs bg-muted text-foreground">{`${name} echoed this`}</span>}
                      </p>
                    </div>
                  ))}
                </section>
              );
            })}

            <section className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-2">
              <h3 className="text-lg font-semibold text-foreground">Fillers and fluency</h3>
              <p className="text-sm text-muted-foreground">
                {fillers.length ? fillers.map(([word, n]) => `${word} ${n}`).join(' · ') : 'No fillers counted'}
              </p>
            </section>

            {report.version >= 2 && (
              <section className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-3">
                <h3 className="text-lg font-semibold text-foreground">Pronunciation hints (may be off)</h3>
                {report.pronunciation.length ? report.pronunciation.map((hint, i) => (
                  <div key={i} className="flex flex-wrap items-center gap-2">
                    <span className="font-medium">{hint.word}</span>
                    {hintPlayButton(detail, hint)}
                    <button type="button" className="rounded px-1.5 py-0.5 hover:bg-accent" onClick={() => speak(hint.word)}>▶ reference</button>
                  </div>
                )) : (
                  <p className="text-sm text-muted-foreground">{report.pronunciation_note ?? 'No pronunciation hints this time'}</p>
                )}
              </section>
            )}

            {showRecurring && (
              <section className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-3">
                <h3 className="text-lg font-semibold text-foreground">Keeps coming back</h3>
                {patterns.length > 0 && (
                  <div className="flex flex-wrap gap-2">
                    {patterns.map((p) => (
                      <span key={p.key} className="rounded px-2 py-0.5 text-sm bg-muted text-foreground">{`${p.key} ×${p.sessions} sessions`}</span>
                    ))}
                  </div>
                )}
                {trend.length >= 2 && (
                  <p className="text-sm text-muted-foreground">
                    <span className="font-medium text-foreground">Fillers per minute</span>{' '}
                    <span className="tabular-nums">{trend.map((v) => v.toFixed(1)).join(' → ')}</span>
                  </p>
                )}
              </section>
            )}
          </>
        ) : s.report_status === 'failed' ? (
          <div className="space-y-3">
            <p className="text-sm text-destructive">{detail.report_error || 'The report could not be written'}</p>
            {generate('Retry')}
          </div>
        ) : s.report_status === 'running' ? (
          <p className="text-muted-foreground">Writing your report…</p>
        ) : s.report_status === 'pending' ? (
          // an interrupted session stays pending: nothing has written a report yet
          s.end_reason === null ? (
            <p className="text-muted-foreground">Finishing the session…</p>
          ) : (
            <div className="space-y-3">
              <p className="text-muted-foreground">Report not generated yet</p>
              {generate('Generate report')}
            </div>
          )
        ) : s.report_status === 'waiting' ? (
          <p className="text-muted-foreground">Waiting for the meeting recording to finish…</p>
        ) : (
          <p className="flex flex-wrap items-center gap-2 text-muted-foreground">
            {'The report is missing — '}
            {generate('Generate report')}
          </p>
        )}
      </div>
    </div>
  );
}
