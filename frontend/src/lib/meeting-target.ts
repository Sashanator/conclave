// Where a search hit or other link lands on the meeting page.
export type MeetingTarget = { id: string; line?: string; tab?: 'summary' | 'notes' };

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
  return target;
}

export function hitTarget(hit: SearchHit): MeetingTarget {
  if (hit.source === 'transcript' && hit.transcriptId) return { id: hit.meetingId, line: hit.transcriptId };
  if (hit.source === 'summary') return { id: hit.meetingId, tab: 'summary' };
  if (hit.source === 'notes') return { id: hit.meetingId, tab: 'notes' };
  return { id: hit.meetingId };
}
