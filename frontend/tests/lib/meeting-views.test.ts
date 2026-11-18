import { describe, expect, test } from 'bun:test';
import {
  arrangeRows, BUILTIN_VIEW_DEFAULTS, effectiveHidden, parseViewConfig, viewReducer, weekRange,
} from '@/lib/meeting-views';
import type { MeetingRow } from '@/lib/meetings';

const row = (id: string, startedAt: string, o: Partial<MeetingRow> = {}): MeetingRow => ({
  id, title: id, titleSource: 'user', templateId: null, emoji: null, reviewed: false, gist: null,
  startedAt, endedAt: null, createdAt: startedAt, calendarUid: null, calendarStart: null, calendarEnd: null,
  calendarCancelledAt: null, hasTranscript: true, hasSummary: true, prepPoints: 0, hasPrep: false, ...o,
});
const d = (day: number, h: number, m = 0) => new Date(2026, 10, day, h, m).toISOString();
const now = new Date(2026, 10, 18, 12);
const labels = (s: { label: string | null }[]) => s.map((x) => x.label);

describe('parseViewConfig', () => {
  test('defaults and fallbacks', () => {
    expect(parseViewConfig('week', '{}')).toEqual(BUILTIN_VIEW_DEFAULTS.week);
    expect(parseViewConfig('all', 'not json')).toEqual(BUILTIN_VIEW_DEFAULTS.all);
    const c = parseViewConfig('v1', '{"group":"weird","density":"compact"}');
    expect(c.group).toBe('none');
    expect(c.density).toBe('compact');
  });
  test('garbage range is dropped', () => {
    const bad = (range: unknown) => parseViewConfig('v1', JSON.stringify({ filter: { range } })).filter.range;
    expect(bad({ from: 'x', to: '2026-11-17' })).toBeUndefined();
    expect(bad({ from: '2026-02-30', to: '2026-03-01' })).toBeUndefined();
    expect(bad({ from: '2026-11-01', to: '2026-11-17' })).toEqual({ from: '2026-11-01', to: '2026-11-17' });
  });
  test('sanitises widths and hidden', () => {
    const c = parseViewConfig('v1', '{"widths":{"date":9999,"x":1},"hidden":["name","summary","zzz"]}');
    expect(c.widths).toEqual({ date: 640 });
    expect(c.hidden).toEqual(['summary']);
  });
});

describe('viewReducer', () => {
  const c = parseViewConfig('v1', '{}');
  test('width, hidden', () => {
    expect(viewReducer(c, { type: 'setWidth', column: 'name', width: 20 }).widths.name).toBe(80);
    const once = viewReducer(c, { type: 'toggleHidden', column: 'summary' });
    expect(viewReducer(once, { type: 'toggleHidden', column: 'summary' }).hidden).toEqual([]);
    expect(viewReducer(c, { type: 'toggleHidden', column: 'name' })).toBe(c);
  });
  test('effectiveHidden adds category when grouped by category', () => {
    expect(effectiveHidden(BUILTIN_VIEW_DEFAULTS.all)).toEqual(['category']);
    expect(effectiveHidden(BUILTIN_VIEW_DEFAULTS.week)).toEqual([]);
  });
});

test('weekRange', () => {
  expect(weekRange(new Date(2026, 10, 22, 23))).toEqual({ from: new Date(2026, 10, 16), to: new Date(2026, 10, 23) });
});

describe('arrangeRows', () => {
  const rows = [
    row('a', d(16, 10)), row('b', d(16, 14, 2)), row('c', d(17, 18)), row('next', d(23, 9)),
  ];
  test('week view groups by day, drops next week', () => {
    const s = arrangeRows(rows, BUILTIN_VIEW_DEFAULTS.week, { liveId: null, now, templates: [] });
    expect(labels(s)).toEqual(['Monday 16 Nov', 'Tuesday 17 Nov']);
    expect(s[0].rows.map((r) => r.id)).toEqual(['a', 'b']);
  });
  test('live row alone in Now, even when filtered out', () => {
    const s = arrangeRows(rows, BUILTIN_VIEW_DEFAULTS.week, { liveId: 'next', now, templates: [] });
    expect(labels(s)).toEqual(['Now', 'Monday 16 Nov', 'Tuesday 17 Nov']);
    expect(s[0].rows.map((r) => r.id)).toEqual(['next']);
  });
  test('category grouping', () => {
    const t = [{ id: 'one_to_one', name: '1:1', emoji: '👥' }, { id: 'interview', name: 'Interview', emoji: '🤝' }];
    const rs = [row('x', d(16, 9), { templateId: 'interview' }), row('y', d(16, 8), { templateId: 'one_to_one' }), row('z', d(16, 7))];
    const s = arrangeRows(rs, BUILTIN_VIEW_DEFAULTS.all, { liveId: null, now, templates: t });
    expect(labels(s)).toEqual(['👥 1:1', '🤝 Interview', 'No template']);
  });
  test('unreviewed needs a summary', () => {
    const rs = [row('s', d(16, 9)), row('n', d(16, 8), { hasSummary: false }), row('r', d(16, 7), { reviewed: true })];
    const s = arrangeRows(rs, BUILTIN_VIEW_DEFAULTS.unreviewed, { liveId: null, now, templates: [] });
    expect(s[0].rows.map((r) => r.id)).toEqual(['s']);
  });
  test('unknown template id filters as no template', () => {
    const rs = [row('u', d(16, 9), { templateId: 'gone' }), row('k', d(16, 8), { templateId: 'interview' })];
    const cfg = { ...BUILTIN_VIEW_DEFAULTS.all, group: 'none' as const, filter: { categories: [null] } };
    const s = arrangeRows(rs, cfg, { liveId: null, now, templates: [{ id: 'interview', name: 'Interview', emoji: null }] });
    expect(s[0].rows.map((r) => r.id)).toEqual(['u']);
  });
  test('custom range includes the to day', () => {
    const cfg = { ...BUILTIN_VIEW_DEFAULTS.all, group: 'none' as const, filter: { range: { from: '2026-11-17', to: '2026-11-17' } } };
    const s = arrangeRows(rows, cfg, { liveId: null, now, templates: [] });
    expect(s[0].rows.map((r) => r.id)).toEqual(['c']);
  });
  test('category sort, no template last', () => {
    const t = [{ id: 'b', name: 'Beta', emoji: null }, { id: 'a', name: 'Alpha', emoji: null }];
    const rs = [row('n', d(16, 9)), row('x', d(16, 9), { templateId: 'b' }), row('y', d(16, 9), { templateId: 'a' })];
    const cfg = { ...BUILTIN_VIEW_DEFAULTS.all, group: 'none' as const, sort: [{ id: 'category' as const, desc: false }] };
    expect(arrangeRows(rs, cfg, { liveId: null, now, templates: t })[0].rows.map((r) => r.id)).toEqual(['y', 'x', 'n']);
  });
  test('states and calendar filters, ties by id', () => {
    const rs = [
      row('c1', d(16, 9), { hasTranscript: false, calendarUid: 'u', calendarCancelledAt: d(15, 1) }),
      row('f', d(16, 9)),
    ];
    const cfg = { ...BUILTIN_VIEW_DEFAULTS.all, group: 'none' as const };
    const go = (filter: object) => arrangeRows(rs, { ...cfg, filter }, { liveId: null, now, templates: [] });
    expect(go({ states: ['cancelled'] })[0].rows.map((r) => r.id)).toEqual(['c1']);
    expect(go({ calendar: true })[0].rows.map((r) => r.id)).toEqual(['c1']);
    expect(go({})[0].rows.map((r) => r.id)).toEqual(['c1', 'f']);
  });
});
