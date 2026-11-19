import { addDays, format, startOfDay, startOfWeek } from 'date-fns';
import { formatDayLabel } from './dates';
import { meetingDate, meetingState, type MeetingRow, type MeetingState } from './meetings';

export type ViewFilter = {
  categories?: (string | null)[];
  reviewed?: boolean;
  states?: MeetingState[];
  calendar?: boolean;
  /** `from`/`to` are `yyyy-MM-dd` dates, both days included. */
  range?: 'this-week' | { from: string; to: string };
};
export type SortId = 'date' | 'name' | 'category' | 'reviewed';
export type ViewGroup = 'none' | 'day' | 'category';
export type ViewConfig = {
  sort: { id: SortId; desc: boolean }[];
  filter: ViewFilter;
  group: ViewGroup;
  widths: Record<string, number>;
  hidden: string[];
  density: 'comfortable' | 'compact';
};
export type Section = { key: string; label: string | null; rows: MeetingRow[] };
export type CategoryInfo = { id: string; name: string; emoji?: string | null };

export const COLUMNS = ['name', 'date', 'category', 'summary', 'reviewed'];
const SORT_IDS: SortId[] = ['date', 'name', 'category', 'reviewed'];
const GROUPS: ViewGroup[] = ['none', 'day', 'category'];
const STATES: MeetingState[] = ['recording', 'finished', 'cancelled', 'not-recorded', 'new'];

const base: ViewConfig = {
  sort: [{ id: 'date', desc: true }],
  filter: {},
  group: 'none',
  widths: {},
  hidden: [],
  density: 'comfortable',
};
export const BUILTIN_VIEW_DEFAULTS: Record<string, ViewConfig> = {
  all: { ...base, group: 'category' },
  week: { ...base, sort: [{ id: 'date', desc: false }], filter: { range: 'this-week' }, group: 'day' },
  unreviewed: { ...base, filter: { reviewed: false } },
};

const clampWidth = (w: number) => Math.min(640, Math.max(80, w));
const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);

/** `yyyy-MM-dd` that is a real calendar date. */
const validDay = (v: unknown): v is string => {
  if (typeof v !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(v)) return false;
  const [y, m, d] = v.split('-').map(Number);
  const t = new Date(y, m - 1, d);
  return t.getFullYear() === y && t.getMonth() === m - 1 && t.getDate() === d;
};

function cleanFilter(v: unknown): ViewFilter | null {
  if (!isObj(v)) return null;
  const f: ViewFilter = {};
  if (Array.isArray(v.categories)) f.categories = v.categories.filter((c) => c === null || typeof c === 'string');
  if (typeof v.reviewed === 'boolean') f.reviewed = v.reviewed;
  if (Array.isArray(v.states)) f.states = v.states.filter((s): s is MeetingState => STATES.includes(s));
  if (typeof v.calendar === 'boolean') f.calendar = v.calendar;
  if (v.range === 'this-week') f.range = 'this-week';
  else if (isObj(v.range) && validDay(v.range.from) && validDay(v.range.to)) {
    f.range = { from: v.range.from, to: v.range.to };
  }
  return f;
}

/** Unknown or broken saved JSON falls back field by field to the view's defaults. */
export function parseViewConfig(id: string, json: string): ViewConfig {
  const d = BUILTIN_VIEW_DEFAULTS[id] ?? base;
  const out: ViewConfig = { ...d, sort: [...d.sort], filter: { ...d.filter }, widths: {}, hidden: [] };
  let raw: unknown;
  try {
    raw = JSON.parse(json);
  } catch {
    return out;
  }
  if (!isObj(raw)) return out;
  if (Array.isArray(raw.sort)) {
    const sort = raw.sort
      .filter((s) => isObj(s) && SORT_IDS.includes(s.id as SortId))
      .map((s) => ({ id: (s as { id: SortId }).id, desc: (s as { desc?: unknown }).desc === true }));
    if (sort.length) out.sort = sort;
  }
  const filter = cleanFilter(raw.filter);
  if (filter) out.filter = filter;
  if (GROUPS.includes(raw.group as ViewGroup)) out.group = raw.group as ViewGroup;
  if (isObj(raw.widths)) {
    for (const [k, w] of Object.entries(raw.widths)) {
      if (COLUMNS.includes(k) && typeof w === 'number' && Number.isFinite(w)) out.widths[k] = clampWidth(w);
    }
  }
  if (Array.isArray(raw.hidden)) out.hidden = raw.hidden.filter((c) => COLUMNS.includes(c) && c !== 'name');
  if (raw.density === 'compact' || raw.density === 'comfortable') out.density = raw.density;
  return out;
}

export type ViewAction =
  | { type: 'setWidth'; column: string; width: number }
  | { type: 'toggleHidden'; column: string }
  | { type: 'setSort'; sort: ViewConfig['sort'] }
  | { type: 'setFilter'; filter: ViewFilter }
  | { type: 'setGroup'; group: ViewGroup }
  | { type: 'setDensity'; density: ViewConfig['density'] };

export function viewReducer(c: ViewConfig, a: ViewAction): ViewConfig {
  switch (a.type) {
    case 'setWidth':
      return COLUMNS.includes(a.column) ? { ...c, widths: { ...c.widths, [a.column]: clampWidth(a.width) } } : c;
    case 'toggleHidden':
      if (a.column === 'name' || !COLUMNS.includes(a.column)) return c;
      return {
        ...c,
        hidden: c.hidden.includes(a.column) ? c.hidden.filter((h) => h !== a.column) : [...c.hidden, a.column],
      };
    case 'setSort':
      return { ...c, sort: a.sort };
    case 'setFilter':
      return { ...c, filter: a.filter };
    case 'setGroup':
      return { ...c, group: a.group };
    case 'setDensity':
      return { ...c, density: a.density };
  }
}

/** Monday 00:00 (included) to next Monday 00:00 (excluded). */
export function weekRange(now: Date): { from: Date; to: Date } {
  const from = startOfWeek(now, { weekStartsOn: 1 });
  return { from, to: addDays(from, 7) };
}

/** The category column is redundant while rows are grouped by category. */
export function effectiveHidden(c: ViewConfig): string[] {
  return c.group === 'category' && !c.hidden.includes('category') ? [...c.hidden, 'category'] : c.hidden;
}

const parseDay = (s: string) => {
  const [y, m, d] = s.split('-').map(Number);
  return new Date(y, m - 1, d);
};

function matches(row: MeetingRow, f: ViewFilter, liveId: string | null, now: Date, known: Set<string>): boolean {
  // An unknown template id is shown as "No template", so it filters as null too.
  const cat = row.templateId !== null && known.has(row.templateId) ? row.templateId : null;
  if (f.categories && !f.categories.includes(cat)) return false;
  if (f.reviewed === false && !(row.hasSummary && !row.reviewed)) return false;
  if (f.reviewed === true && !row.reviewed) return false;
  if (f.states && !f.states.includes(meetingState(row, liveId, now))) return false;
  if (f.calendar !== undefined && (row.calendarUid !== null) !== f.calendar) return false;
  if (f.range) {
    const { from, to } =
      f.range === 'this-week' ? weekRange(now) : { from: parseDay(f.range.from), to: addDays(parseDay(f.range.to), 1) };
    const t = meetingDate(row).getTime();
    if (t < from.getTime() || t >= to.getTime()) return false;
  }
  return true;
}

export function arrangeRows(
  rows: MeetingRow[],
  config: ViewConfig,
  { liveId, now, templates }: { liveId: string | null; now: Date; templates: CategoryInfo[] },
): Section[] {
  const tName = (id: string | null) => templates.find((t) => t.id === id)?.name ?? '';
  const live = liveId ? rows.find((r) => r.id === liveId) : undefined;
  const known = new Set(templates.map((t) => t.id));
  const rest = rows.filter((r) => r.id !== live?.id && matches(r, config.filter, liveId, now, known));

  const cmp = (a: MeetingRow, b: MeetingRow) => {
    for (const { id, desc } of config.sort) {
      let r = 0;
      if (id === 'date') r = meetingDate(a).getTime() - meetingDate(b).getTime();
      else if (id === 'name') r = a.title.localeCompare(b.title);
      else if (id === 'category') {
        // No template sorts last when ascending.
        const [x, y] = [tName(a.templateId), tName(b.templateId)];
        r = !x !== !y ? (x ? -1 : 1) : x.localeCompare(y);
      } else r = Number(a.reviewed) - Number(b.reviewed);
      if (r) return desc ? -r : r;
    }
    return a.id.localeCompare(b.id);
  };
  rest.sort(cmp);

  const sections: Section[] = [];
  if (live) sections.push({ key: 'now', label: 'Now', rows: [live] });

  if (config.group === 'none') {
    if (rest.length) sections.push({ key: 'all', label: null, rows: rest });
  } else if (config.group === 'day') {
    const byDay = new Map<string, Section>();
    for (const r of rest) {
      const d = startOfDay(meetingDate(r));
      const key = format(d, 'yyyy-MM-dd');
      if (!byDay.has(key)) byDay.set(key, { key, label: formatDayLabel(d, now), rows: [] });
      byDay.get(key)!.rows.push(r);
    }
    // Days follow the date sort direction; any other primary sort reads oldest day first.
    const desc = config.sort[0]?.id === 'date' && config.sort[0].desc;
    sections.push(...[...byDay.values()].sort((a, b) => (desc ? -1 : 1) * a.key.localeCompare(b.key)));
  } else {
    const groups = templates.map((t) => ({
      key: t.id,
      label: t.emoji ? `${t.emoji} ${t.name}` : t.name,
      rows: rest.filter((r) => r.templateId === t.id),
    }));
    groups.push({
      key: 'none',
      label: 'No template',
      rows: rest.filter((r) => r.templateId === null || !known.has(r.templateId)),
    });
    sections.push(...groups.filter((g) => g.rows.length));
  }
  return sections;
}

/** Keys of the sections that start closed: with day grouping only today's (else the first) day is open. */
export function defaultCollapsed(sections: Section[], group: ViewGroup, now: Date): Set<string> {
  if (group !== 'day') return new Set();
  const days = sections.map((s) => s.key).filter((k) => k !== 'now');
  const open = days.includes(format(now, 'yyyy-MM-dd')) ? format(now, 'yyyy-MM-dd') : days[0];
  return new Set(days.filter((k) => k !== open));
}
