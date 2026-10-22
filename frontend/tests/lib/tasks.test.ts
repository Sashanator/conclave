import { describe, expect, test } from 'bun:test';
import { groupByMeeting, type TaskRow } from '../../src/lib/tasks';

const row = (over: Partial<TaskRow>): TaskRow => ({
  id: 'x',
  meetingId: null,
  meetingTitle: null,
  meetingDate: null,
  text: 'Task',
  owner: 'me',
  due: null,
  at: null,
  status: 'open',
  source: 'auto',
  createdAt: 't',
  ...over,
});

describe('groupByMeeting', () => {
  test('keeps the row order per meeting and puts the manual group last', () => {
    const groups = groupByMeeting([
      row({ id: '1', meetingId: 'm2', meetingTitle: 'Second', meetingDate: '2026-10-21' }),
      row({ id: '2', source: 'manual', text: 'Mine' }),
      row({ id: '3', meetingId: 'm1', meetingTitle: 'First', meetingDate: '2026-10-20' }),
      row({ id: '4', meetingId: 'm2', meetingTitle: 'Second', meetingDate: '2026-10-21' }),
    ]);
    expect(groups.map((g) => g.title)).toEqual(['Second', 'First', 'Your own tasks']);
    expect(groups[0].tasks.map((t) => t.id)).toEqual(['1', '4']);
    expect(groups[0]).toMatchObject({ meetingId: 'm2', date: '2026-10-21' });
    expect(groups[2]).toMatchObject({ meetingId: null, date: null });
    expect(groups[2].tasks.map((t) => t.id)).toEqual(['2']);
  });

  test('a manual task listed first still forms the last group', () => {
    const groups = groupByMeeting([
      row({ id: 'm', source: 'manual' }),
      row({ id: 'a', meetingId: 'm1', meetingTitle: 'One' }),
    ]);
    expect(groups.map((g) => g.title)).toEqual(['One', 'Your own tasks']);
  });

  test('no manual tasks means no manual group', () => {
    expect(groupByMeeting([row({ id: 'a', meetingId: 'm1', meetingTitle: 'One' })]).map((g) => g.title)).toEqual(['One']);
    expect(groupByMeeting([])).toEqual([]);
  });
});
