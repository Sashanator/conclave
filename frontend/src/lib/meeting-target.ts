// Where a search hit or other link lands on the meeting page. `t` is seconds into the recording.
export type MeetingTarget = { id: string; line?: string; tab?: 'summary' | 'notes'; t?: number };

// Camel-case shape of the Rust `SearchHit` returned by `api_search_transcripts`.
export type SearchHit = {
  meetingId: string;
  title: string;
  source: 'title' | 'transcript' | 'summary' | 'notes';
  snippet: string;
  transcriptId: string | null;
  audioStart: number | null;
};

export function meetingHref(t: MeetingTarget): string {
  const params = new URLSearchParams({ id: t.id });
  if (t.line !== undefined) params.set('line', t.line);
  if (t.tab !== undefined) params.set('tab', t.tab);
  if (t.t !== undefined) params.set('t', String(t.t));
  return `/meeting-details?${params.toString()}`;
}

export function parseMeetingTarget(p: URLSearchParams): MeetingTarget | null {
  const id = p.get('id');
  if (!id) return null;
  const target: MeetingTarget = { id };
  const line = p.get('line');
  if (line) target.line = line;
  const tab = p.get('tab');
  if (tab === 'summary' || tab === 'notes') target.tab = tab;
  const time = Number(p.get('t'));
  if (p.get('t') && Number.isFinite(time) && time >= 0) target.t = time;
  return target;
}

/** The row a time points at: the last row starting at or before `t`. Null while the row may be on a page not loaded yet. */
export function lineAtTime(rows: { id: string; timestamp: number }[], t: number, hasMore: boolean): string | null {
  if (rows.length === 0) return null;
  if (hasMore && rows[rows.length - 1].timestamp <= t) return null;
  let found = rows[0].id;
  for (const row of rows) {
    if (row.timestamp > t) break;
    found = row.id;
  }
  return found;
}

export function hitTarget(hit: SearchHit): MeetingTarget {
  if (hit.source === 'transcript' && hit.transcriptId) return { id: hit.meetingId, line: hit.transcriptId };
  if (hit.source === 'summary') return { id: hit.meetingId, tab: 'summary' };
  if (hit.source === 'notes') return { id: hit.meetingId, tab: 'notes' };
  return { id: hit.meetingId };
}
