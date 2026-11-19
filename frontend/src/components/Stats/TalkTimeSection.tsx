'use client';

import { Fragment, useEffect, useState } from 'react';
import { ChevronRight } from 'lucide-react';
import Link from 'next/link';
import { invoke } from '@tauri-apps/api/core';
import { formatLongDate } from '@/lib/dates';
import { rememberOrigin } from '@/lib/back-nav';
import { sharePct, type MeetingShare, type Period, type TalkTime, type TypeShare } from '@/lib/stats';

// "No type" meetings have a null template_id, so the key is '' for them
const typeKey = (t: TypeShare) => t.template_id ?? '';

// A YYYY-MM-DD from Rust is a local day; anything else is shown as given
const dayLabel = (v: string) => {
  const d = new Date(`${v}T00:00`);
  return Number.isNaN(d.getTime()) ? v : formatLongDate(d);
};

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

  const share = (t: TypeShare | undefined) => (t && t.total_seconds ? Math.round((t.self_seconds / t.total_seconds) * 100) : 0);
  const prevLabel = period === 'week' ? 'Previous week' : 'Previous month';
  const th = 'px-3 py-2 text-left font-medium text-muted-foreground';

  return (
    <section aria-labelledby="stats-talk" className="space-y-4">
      <h2 id="stats-talk" className="text-xl font-semibold">Talk time</h2>
      <div className="overflow-x-auto rounded-lg border border-border bg-card">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-border">
              <th className={th}>Category</th>
              <th className={th}>Meetings</th>
              <th className={th}>
                Your share of talk time
                <span className="block text-xs font-normal text-muted-foreground">how much of the talking was you</span>
              </th>
              <th className={th}>{prevLabel}</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-border">
            {[...types.values()].map((t) => {
              const now = find(talk.current, t);
              const before = find(talk.previous, t);
              const open = !!selected && typeKey(selected) === typeKey(t);
              return (
                <Fragment key={typeKey(t)}>
                  <tr>
                    <td className="px-3 py-2">
                      <button
                        type="button"
                        aria-expanded={open}
                        onClick={() => setSelected(open ? null : t)}
                        className="flex items-center gap-1 font-medium hover:underline"
                      >
                        <ChevronRight className={`h-4 w-4 transition-transform ${open ? 'rotate-90' : ''}`} aria-hidden />
                        {t.template_name}
                      </button>
                    </td>
                    <td className="px-3 py-2 text-muted-foreground">{`${now?.meetings ?? 0} ${now?.meetings === 1 ? 'meeting' : 'meetings'}`}</td>
                    <td className="px-3 py-2">
                      <div className="flex items-center gap-3">
                        <span className="w-10 shrink-0">{now ? sharePct(now.self_seconds, now.total_seconds) : '—'}</span>
                        <div className="h-2 min-w-16 flex-1 rounded bg-muted">
                          <div className="h-2 rounded bg-primary" style={{ width: `${share(now)}%` }} />
                        </div>
                      </div>
                    </td>
                    <td className="px-3 py-2 text-muted-foreground">{before ? sharePct(before.self_seconds, before.total_seconds) : '—'}</td>
                  </tr>
                  {open && (
                    <tr>
                      <td colSpan={4} className="px-6 pb-3">
                        <ul className="space-y-1">
                          {failed && <li className="text-red-600">Could not load these meetings</li>}
                          {!failed && meetings === null && <li className="text-muted-foreground">Loading…</li>}
                          {meetings?.map((m) => (
                            <li key={m.meeting_id} className="flex items-center justify-between gap-4">
                              <Link
                                href={`/meeting-details?id=${m.meeting_id}`}
                                onClick={() => rememberOrigin({ path: '/stats', label: 'Stats' })}
                                className="truncate hover:underline"
                              >
                                {m.title}
                              </Link>
                              <span className="text-muted-foreground whitespace-nowrap">{`${dayLabel(m.date)} · ${sharePct(m.self_seconds, m.total_seconds)}`}</span>
                            </li>
                          ))}
                        </ul>
                      </td>
                    </tr>
                  )}
                </Fragment>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}
