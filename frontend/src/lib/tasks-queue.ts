// Pure logic of the Tasks review queue: grouping, counts, what can be sent, and the copy around it.
import { format, parseISO } from 'date-fns';
import { parseDbTime } from './meetings';
import { BLOCKING_STATES, DEST_STATE_LABEL, type DestState, type Destination, type SendReport } from './notion';
import type { TaskRow } from './tasks';

export const TASKS_CHANGED_EVENT = 'tasks-changed';
export const DELETED_UNKNOWN = 'Deleted meeting (title unknown)';

export type QueueGroup = { key: string; title: string; meetingId: string | null; date: string | null; templateId: string | null; tasks: TaskRow[] };
export type DestLookup = { dests: Destination[]; tokenSet: boolean };
export type SendCheck =
  | { kind: 'notKept' }
  | { kind: 'ambiguous' }
  | { kind: 'needDb' }
  | { kind: 'waiting'; dest: Destination; state: DestState }
  | { kind: 'sendable'; dest: Destination };
export type SendSummary = {
  kept: number;
  byDest: { dest: Destination; n: number }[];
  needDb: number;
  waiting: number;
  waitingNames: string[];
  undecided: number;
  sendableIds: string[];
};

export const isToReview = (t: TaskRow) => t.review === 'open' || t.review === 'keep' || t.review === 'sending';
export const toReviewCount = (tasks: TaskRow[]) => tasks.filter(isToReview).length;

/** Newest meeting first (no date last); rows keep their input order inside a group. */
export function groupQueue(tasks: TaskRow[]): QueueGroup[] {
  const groups = new Map<string, QueueGroup>();
  for (const t of tasks) {
    const title = t.meetingId
      ? (t.meetingTitle ?? 'Untitled meeting')
      : t.meetingTitle ? `Deleted meeting: ${t.meetingTitle}` : DELETED_UNKNOWN;
    const key = t.meetingId ? `m:${t.meetingId}` : `d:${title}|${t.meetingDate}`;
    const g = groups.get(key);
    if (g) g.tasks.push(t);
    else groups.set(key, { key, title, meetingId: t.meetingId, date: t.meetingDate, templateId: t.templateId, tasks: [t] });
  }
  const ms = (g: QueueGroup) => (g.date ? parseDbTime(g.date).getTime() : -Infinity);
  return [...groups.values()].sort((a, b) => ms(b) - ms(a));
}

export const destOf = (t: TaskRow, dests: Destination[]) => dests.find((d) => d.id === t.destId) ?? null;

export function sendCheck(t: TaskRow, { dests, tokenSet }: DestLookup): SendCheck {
  if (t.review !== 'keep') return { kind: 'notKept' };
  if (t.sendAmbiguous) return { kind: 'ambiguous' };
  const dest = destOf(t, dests);
  if (!dest) return { kind: 'needDb' };
  if (!tokenSet) return { kind: 'waiting', dest, state: 'token_missing' };
  if (dest.lastStatus && BLOCKING_STATES.includes(dest.lastStatus)) return { kind: 'waiting', dest, state: dest.lastStatus };
  return { kind: 'sendable', dest };
}

export function sendSummary(tasks: TaskRow[], lookup: DestLookup): SendSummary {
  const s: SendSummary = { kept: 0, byDest: [], needDb: 0, waiting: 0, waitingNames: [], undecided: 0, sendableIds: [] };
  const counts = new Map<string, { dest: Destination; n: number }>();
  for (const t of tasks) {
    if (t.review === 'open') s.undecided++;
    if (t.review === 'keep') s.kept++;
    const c = sendCheck(t, lookup);
    if (c.kind === 'needDb') s.needDb++;
    else if (c.kind === 'waiting') {
      s.waiting++;
      if (!s.waitingNames.includes(c.dest.name)) s.waitingNames.push(c.dest.name);
    } else if (c.kind === 'sendable') {
      s.sendableIds.push(t.id);
      const e = counts.get(c.dest.id);
      if (e) e.n++;
      else counts.set(c.dest.id, { dest: c.dest, n: 1 });
    }
  }
  s.byDest = [...counts.values()].sort((a, b) => a.dest.position - b.dest.position);
  return s;
}

export const sendButtonLabel = (n: number, sending: boolean) => (sending ? 'Sending…' : n > 0 ? `Send ${n} to Notion` : 'Nothing to send');

export function resultLine(r: SendReport): { sent: string | null; notSent: string | null; aiNote: string | null } {
  const bad = r.failed + r.ambiguous;
  return { sent: r.sent > 0 ? `Sent ${r.sent} to Notion` : null, notSent: bad > 0 ? `${bad} not sent, see To review` : null,
    aiNote: r.aiNotFilled?.length ? `AI couldn't fill the other columns (${r.aiNotFilled.join(', ')}).` : null };
}

/** After a reload, the row to focus: the wanted one, else the next surviving one, else the previous one. */
export function pickFocus(before: string[], after: string[], wanted: string): string | null {
  if (after.includes(wanted)) return wanted;
  const i = before.indexOf(wanted);
  const next = before.slice(i + 1).find((id) => after.includes(id));
  if (i >= 0 && next) return next;
  const prev = before.slice(0, Math.max(i, 0)).reverse().find((id) => after.includes(id));
  return prev ?? after[0] ?? null;
}

/** Key 1..9 picks the n-th destination by position. */
export function destForKey(key: string, dests: Destination[]): Destination | null {
  if (!/^[1-9]$/.test(key)) return null;
  return [...dests].sort((a, b) => a.position - b.position)[Number(key) - 1] ?? null;
}

export function dueLabel(t: Pick<TaskRow, 'dueDate' | 'due'>): string {
  if (t.dueDate) return `by ${format(parseISO(t.dueDate), 'EEE d MMM')}`; // date-only, so no zone shift
  return t.due ? `by ${t.due}` : 'no due date';
}

export function waitingText(name: string, state: DestState): string {
  return state === 'not_shared'
    ? `${name} isn't shared with Conclave yet, so this task waits.`
    : `${name} isn't connected (${DEST_STATE_LABEL[state]}), so this task waits.`;
}

export function isEditingText(el: Element | null): boolean {
  if (!el) return false;
  const ce = el.getAttribute('contenteditable');
  return el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || ce === '' || ce === 'true';
}
