'use client';

import { useEffect, useRef, useState } from 'react';
import Link from 'next/link';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { groupHits, meetingHref, snippetHref, snippetLabel, type SearchHit } from '@/lib/meeting-target';
import { formatMeetingDate, type MeetingRow } from '@/lib/meetings';
import type { CategoryInfo } from '@/lib/meeting-views';
import { CategoryChip, ReviewedCell } from './cells';

/** Debounced search; only the newest request may write results. `hits` is null while not searching. */
export function useMeetingSearch(query: string) {
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  const seq = useRef(0);
  useEffect(() => {
    const mine = ++seq.current;
    const q = query.trim();
    if (!q) { setHits(null); return; }
    const t = setTimeout(async () => {
      try {
        const found = await invoke<SearchHit[]>('api_search_transcripts', { query: q });
        if (mine === seq.current) setHits(found);
      } catch (e) {
        if (mine === seq.current) toast.error('Search failed', { description: e instanceof Error ? e.message : String(e) });
      }
    }, 250);
    return () => clearTimeout(t);
  }, [query]);
  return hits;
}

const th = 'px-[10px] py-1 text-left text-xs font-semibold text-muted-foreground';

export function SearchResults({ query, hits, rows, templates, now, selectedId, onClear, onToggleReviewed, onChangeTemplate }: {
  query: string;
  hits: SearchHit[];
  rows: MeetingRow[];
  templates: CategoryInfo[];
  now: Date;
  selectedId: string | null;
  onClear: () => void;
  onToggleReviewed: (id: string) => void;
  onChangeTemplate: (id: string, templateId: string | null) => void;
}) {
  const groups = groupHits(hits);
  if (!groups.length) {
    return (
      <div role="status" className="py-10 text-center text-sm text-muted-foreground">
        <p>Nothing mentions “{query.trim()}”. Try fewer words or another spelling.</p>
        <button type="button" onClick={onClear} className="mt-2 text-foreground underline">Clear search</button>
      </div>
    );
  }
  return (
    <>
      <p role="status" className="mb-2 inline-block rounded-full bg-muted px-2.5 py-0.5 text-xs">
        Searching all meetings · {groups.length} {groups.length === 1 ? 'match' : 'matches'} · <span className="text-muted-foreground">Esc clears</span>
      </p>
      <table className="w-full table-fixed border-collapse text-sm">
        <thead>
          <tr>
            <th scope="col" className={`${th} w-[45%]`}>Name and where it matched</th>
            <th scope="col" className={th}>Date</th>
            <th scope="col" className={th}>Category</th>
            <th scope="col" className={`${th} w-[90px]`}>Reviewed</th>
          </tr>
        </thead>
        <tbody>
          {groups.map((g) => {
            const row = rows.find((r) => r.id === g.meetingId);
            return (
              <tr key={g.meetingId} data-row-id={g.meetingId} aria-current={selectedId === g.meetingId || undefined}
                className={`border-b align-top ${selectedId === g.meetingId ? 'bg-accent' : 'hover:bg-accent'}`}>
                <td className="px-[10px] py-1.5">
                  <Link href={meetingHref({ id: g.meetingId })} className="font-medium hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
                    {row?.emoji ? <span aria-hidden>{row.emoji} </span> : null}{row?.title ?? g.hits[0].title}
                  </Link>
                  <div className="mt-1 space-y-1">
                    {g.hits.slice(0, 3).map((h, i) => (
                      <Link key={`${h.source}-${h.transcriptId ?? i}`} href={snippetHref(h)}
                        className="flex gap-2 rounded px-1 text-xs text-muted-foreground hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
                        <span className="shrink-0 font-medium text-foreground">{snippetLabel(h)}</span>
                        <span className="line-clamp-2">{h.snippet}</span>
                      </Link>
                    ))}
                    {g.more > 0 && <Link href={meetingHref({ id: g.meetingId })} className="block px-1 text-xs underline">{g.more} more in this meeting</Link>}
                  </div>
                </td>
                <td className="px-[10px] py-1.5 text-muted-foreground">{row ? formatMeetingDate(row, now) : ''}</td>
                <td className="px-[10px] py-1.5">{row && <CategoryChip row={row} templates={templates} onChange={(t) => onChangeTemplate(row.id, t)} />}</td>
                <td className="px-[10px] py-1.5">{row && <ReviewedCell row={row} onToggle={() => onToggleReviewed(row.id)} />}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </>
  );
}
