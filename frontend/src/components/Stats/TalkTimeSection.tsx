'use client';

import { useEffect, useState } from 'react';
import Link from 'next/link';
import { invoke } from '@tauri-apps/api/core';
import { sharePct, type MeetingShare, type Period, type TalkTime, type TypeShare } from '@/lib/stats';

// "No type" meetings have a null template_id, so the key is '' for them
const typeKey = (t: TypeShare) => t.template_id ?? '';

/** The page gives this a key per period, so a type picked in one period does not carry over. */
export function TalkTimeSection({ period, talk }: { period: Period; talk: TalkTime }) {
  const [selected, setSelected] = useState<TypeShare | null>(null);
  const [meetings, setMeetings] = useState<MeetingShare[] | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!selected) return;
    let live = true;
    setMeetings(null);
    setFailed(false);
    invoke<MeetingShare[]>('stats_talk_time_meetings', { period, back: 0, templateId: selected.template_id })
      .then((list) => {
        if (live) setMeetings(list);
      })
      .catch((err) => {
        console.error('Talk-time meetings failed to load:', err);
        if (live) setFailed(true);
      });
    return () => {
      live = false;
    };
  }, [selected, period]);

  const types = new Map<string, TypeShare>();
  for (const t of [...talk.current, ...talk.previous]) {
    if (!types.has(typeKey(t))) types.set(typeKey(t), t);
  }
  const find = (list: TypeShare[], t: TypeShare) => list.find((x) => typeKey(x) === typeKey(t));

  return (
    <section aria-labelledby="stats-talk" className="space-y-4">
      <h2 id="stats-talk" className="text-xl font-semibold">Talk time</h2>
      <ul className="divide-y divide-border rounded-lg border border-border bg-background">
        {[...types.values()].map((t) => {
          const now = find(talk.current, t);
          const before = find(talk.previous, t);
          const open = selected !== null && typeKey(selected) === typeKey(t);
          return (
            <li key={typeKey(t)}>
              <button
                type="button"
                aria-expanded={open}
                onClick={() => setSelected(open ? null : t)}
                className="flex w-full items-center justify-between px-4 py-3 text-left text-sm hover:bg-accent"
              >
                <span className="font-medium">{t.template_name}</span>
                <span className="text-muted-foreground">{`${now?.meetings ?? 0} ${now?.meetings === 1 ? 'meeting' : 'meetings'}`}</span>
                <span>{`Share ${now ? sharePct(now.self_seconds, now.total_seconds) : '—'}`}</span>
                <span className="text-muted-foreground">{`Previous ${before ? sharePct(before.self_seconds, before.total_seconds) : '—'}`}</span>
              </button>
              {open && (
                <ul className="space-y-1 px-6 pb-3 text-sm">
                  {failed && <li className="text-red-600">Could not load these meetings</li>}
                  {!failed && meetings === null && <li className="text-muted-foreground">Loading…</li>}
                  {meetings?.map((m) => (
                    <li key={m.meeting_id} className="flex items-center justify-between gap-4">
                      <Link href={`/meeting-details?id=${m.meeting_id}`} className="truncate hover:underline">
                        {m.title}
                      </Link>
                      <span className="text-muted-foreground whitespace-nowrap">{`${m.date} · ${sharePct(m.self_seconds, m.total_seconds)}`}</span>
                    </li>
                  ))}
                </ul>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
