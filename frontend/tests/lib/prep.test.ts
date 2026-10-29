import { afterAll, describe, expect, mock, test } from 'bun:test';

const originals = { core: { ...(await import('@tauri-apps/api/core')) } };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
});

const invoke = mock(async (_command: string, _args?: unknown): Promise<unknown> => null);
mock.module('@tauri-apps/api/core', () => ({ invoke }));

const {
  approvalLabel,
  charCount,
  countLabel,
  groupByDay,
  prepCounter,
  savePrep,
  tickItem,
} = await import('../../src/lib/prep');

const item = (ticked: boolean) => ({ id: 'i', text: 'x', kind: 'question', ticked, byUser: false });

describe('prepCounter', () => {
  test('counts ticked over total', () => {
    expect(prepCounter([item(true), item(false), item(true), item(false)])).toBe('Prep 2/4');
    expect(prepCounter([])).toBe('Prep 0/0');
  });
});

describe('charCount', () => {
  test('counts code points', () => {
    expect(charCount('😀ab')).toBe(3);
  });
});

describe('countLabel', () => {
  test('formats thousands', () => {
    expect(countLabel(41_300, 40_000)).toBe('41,300 / 40,000 characters');
  });
});

describe('approvalLabel', () => {
  const base = { approvedAt: null, updatedAt: '2026-10-29T10:00:00.000Z' };

  test('null when not approved', () => {
    expect(approvalLabel(base)).toBeNull();
  });

  test('Approved when nothing changed after approval', () => {
    expect(approvalLabel({ approvedAt: '2026-10-29T10:00:00.000Z', updatedAt: '2026-10-29T10:00:00.000Z' })).toBe('Approved');
  });

  test('edited after approval', () => {
    expect(approvalLabel({ approvedAt: '2026-10-29T09:00:00.000Z', updatedAt: '2026-10-29T10:00:00.000Z' })).toBe(
      'Approved · edited after approval',
    );
  });
});

describe('groupByDay', () => {
  const ev = (uid: string, start: Date) => ({ uid, title: uid, start: start.toISOString(), end: start.toISOString(), templateId: null, templateName: null, prep: null });

  test('keeps order by start and groups by local date', () => {
    const groups = groupByDay([
      ev('a', new Date(2026, 9, 29, 9, 0)),
      ev('b', new Date(2026, 9, 29, 15, 0)),
      ev('c', new Date(2026, 9, 31, 8, 0)),
    ]);
    expect(groups.map((g) => g.events.map((e) => e.uid))).toEqual([['a', 'b'], ['c']]);
    expect(groups[0].day).toBe('2026-10-29');
    expect(groups[1].day).toBe('2026-10-31');
  });
});

describe('tickItem', () => {
  test('invokes prep_tick with camelCase args', async () => {
    invoke.mockClear();
    await tickItem(3, 'i', true);
    expect(invoke).toHaveBeenCalledWith('prep_tick', { prepId: 3, itemId: 'i', ticked: true });
  });
});

describe('savePrep', () => {
  test('invokes prep_save with the same argument names', async () => {
    invoke.mockClear();
    const args = {
      eventUid: 'u',
      eventStart: '2026-10-30T09:00:00+00:00',
      eventTitle: 'Interview',
      material: 'Notes',
      templateId: null,
      items: [item(false)],
    };
    await savePrep(args);
    expect(invoke).toHaveBeenCalledWith('prep_save', args);
  });
});
