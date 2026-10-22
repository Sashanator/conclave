import type { MeetingTarget } from '@/lib/meeting-target';

// Camel-case shape of the Rust `Citation` returned inside `AskAnswer`.
export type Citation = {
  id: string;
  meetingId: string;
  meetingTitle: string;
  date: string;
  transcriptId: string | null;
  audioStart: number | null;
  source: 'transcript' | 'summary' | 'notes';
};

// Mirrors the Rust `Turn` sent as `history` to `ask_meetings`.
export type Turn = { role: 'user' | 'assistant'; text: string };

// Mirrors the Rust `AskAnswer`.
export type AskAnswer = { text: string; citations: Citation[] };

export type Part = { kind: 'text'; text: string } | { kind: 'cite'; citation: Citation };

const MARKER = /\[E(\d+)\]/g;
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

let lastRequestId = 0;

/** A new id for each question, so a stale answer or cancel never reaches the wrong turn. */
export function nextAskRequestId(): number {
  lastRequestId += 1;
  return lastRequestId;
}

/** Splits the answer at its [E3] markers. A known id becomes a cite part; an unknown id is dropped. */
export function splitAnswer(text: string, citations: Citation[]): Part[] {
  const byId = new Map(citations.map((c) => [c.id, c]));
  const parts: Part[] = [];
  const addText = (s: string) => {
    if (!s) return;
    const last = parts[parts.length - 1];
    if (last?.kind === 'text') last.text += s;
    else parts.push({ kind: 'text', text: s });
  };
  let last = 0;
  for (const m of text.matchAll(MARKER)) {
    addText(text.slice(last, m.index));
    const citation = byId.get(`E${m[1]}`);
    if (citation) parts.push({ kind: 'cite', citation });
    last = m.index! + m[0].length;
  }
  addText(text.slice(last));
  return parts;
}

/** The meeting page target: a transcript citation opens its line, summary and notes open their tab. */
export function citationTarget(c: Citation): MeetingTarget {
  if (c.source === 'transcript') return { id: c.meetingId, line: c.transcriptId ?? undefined };
  return { id: c.meetingId, tab: c.source === 'notes' ? 'notes' : 'summary' };
}

/** "Title · 20 Oct · 12:04", the time only when the citation has an audio position. */
export function citeLabel(c: Citation): string {
  const [, month, day] = c.date.split('-');
  const parts = [c.meetingTitle, `${Number(day)} ${MONTHS[Number(month) - 1]}`];
  if (c.audioStart !== null) parts.push(clock(c.audioStart));
  return parts.join(' · ');
}

function clock(seconds: number): string {
  const s = Math.floor(seconds);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
}
