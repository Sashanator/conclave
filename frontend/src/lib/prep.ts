// Meeting prep types as the Rust side sends them (frontend/src-tauri/src/prep), camelCase.
// Invoke wrappers use the pinned Tauri command names; helpers are pure.
import { invoke } from '@tauri-apps/api/core';

export interface PrepItem {
  id: string;
  text: string;
  kind: string;
  ticked: boolean;
  byUser: boolean;
}

export interface ChatMessage {
  role: 'user' | 'assistant';
  text: string;
  at: string;
  listNotUpdated?: boolean;
}

export interface MeetingPrep {
  id: number;
  eventUid: string;
  eventStart: string;
  eventTitle: string;
  templateId: string | null;
  material: string;
  chat: ChatMessage[];
  items: PrepItem[];
  approvedAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface PrepReply {
  prep: MeetingPrep;
  listUpdated: boolean;
}

export interface PrepBadge {
  id: number;
  approved: boolean;
}

export interface UpcomingEvent {
  uid: string;
  title: string;
  start: string;
  end: string;
  templateId: string | null;
  templateName: string | null;
  prep: PrepBadge | null;
}

export interface OtherPrep {
  id: number;
  eventUid: string;
  eventStart: string;
  eventTitle: string;
  approved: boolean;
  reason: 'past' | 'not_in_calendar';
}

export interface PrepOverview {
  connected: boolean;
  upcoming: UpcomingEvent[];
  other: OtherPrep[];
}

export const SUGGEST_MESSAGE = 'Suggest what I should ask and clarify';

export const getPrep = (eventUid: string, eventStart: string) =>
  invoke<MeetingPrep | null>('prep_get', { eventUid, eventStart });

export const prepForMeeting = (meetingId: string) =>
  invoke<MeetingPrep | null>('prep_for_meeting', { meetingId });

export const savePrep = (args: {
  eventUid: string;
  eventStart: string;
  eventTitle: string;
  material: string;
  templateId: string | null;
  items: PrepItem[];
}) => invoke<MeetingPrep>('prep_save', args);

export const tickItem = (prepId: number, itemId: string, ticked: boolean) =>
  invoke<MeetingPrep>('prep_tick', { prepId, itemId, ticked });

export const approvePrep = (prepId: number) => invoke<MeetingPrep>('prep_approve', { prepId });

export const deletePrep = (prepId: number) => invoke<void>('prep_delete', { prepId });

export const sendPrep = (prepId: number, message: string) =>
  invoke<PrepReply>('prep_send', { prepId, message });

export const materialLimit = () => invoke<number>('prep_material_limit');

export const prepOverview = () => invoke<PrepOverview>('prep_overview');

export const prepCounter = (items: PrepItem[]) =>
  `Prep ${items.filter((i) => i.ticked).length}/${items.length}`;

// Code points, as Rust's chars().count().
export const charCount = (s: string) => [...s].length;

export const countLabel = (count: number, limit: number) =>
  `${count.toLocaleString('en-US')} / ${limit.toLocaleString('en-US')} characters`;

export const approvalLabel = (prep: Pick<MeetingPrep, 'approvedAt' | 'updatedAt'>): string | null => {
  if (!prep.approvedAt) return null;
  const approved = Date.parse(prep.approvedAt);
  const updated = Date.parse(prep.updatedAt);
  // An unreadable date cannot show an edit, so the plain label is the safe one.
  if (Number.isNaN(approved) || Number.isNaN(updated)) return 'Approved';
  return updated > approved ? 'Approved · edited after approval' : 'Approved';
};

const localDay = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;

// An unreadable start sorts last and keeps its raw text as its day, so no comparator sees NaN.
const startKey = (ev: UpcomingEvent) => {
  const t = Date.parse(ev.start);
  return Number.isNaN(t) ? Number.POSITIVE_INFINITY : t;
};

export const groupByDay = (events: UpcomingEvent[]): Array<{ day: string; events: UpcomingEvent[] }> => {
  const groups: Array<{ day: string; events: UpcomingEvent[] }> = [];
  const sorted = [...events].sort((a, b) => {
    const ka = startKey(a);
    const kb = startKey(b);
    return ka === kb ? 0 : ka < kb ? -1 : 1;
  });
  for (const ev of sorted) {
    const start = new Date(ev.start);
    const day = Number.isNaN(start.getTime()) ? ev.start : localDay(start);
    const last = groups[groups.length - 1];
    if (last && last.day === day) last.events.push(ev);
    else groups.push({ day, events: [ev] });
  }
  return groups;
};
