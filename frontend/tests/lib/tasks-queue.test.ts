import { describe, expect, test } from 'bun:test';
import type { Destination } from '../../src/lib/notion';
import type { TaskRow } from '../../src/lib/tasks';
import {
  destForKey, dueLabel, groupQueue, isEditingText, pickFocus, resultLine, sendButtonLabel, sendCheck, sendSummary, toReviewCount, waitingText,
} from '../../src/lib/tasks-queue';

const t = (over: Partial<TaskRow> = {}): TaskRow => ({
  id: 'x', meetingId: 'm1', meetingTitle: 'M', meetingDate: '2026-11-10 09:00:00+00:00', text: 'Task', owner: 'me', due: null, at: null, status: 'open', source: 'auto',
  createdAt: 't', review: 'open', removedBy: null, destId: null, dueDate: null, notionUrl: null, sentAt: null, sendError: null, sendAmbiguous: false, templateId: null, ...over,
});
const dest = (id: string, name: string, colour: number, position: number, lastStatus: Destination['lastStatus'] = null): Destination => ({
  id, name, colour, position, databaseId: id, dataSourceId: null, mapping: { titleProp: 'Name', dueProp: null, meetingProp: null, fixed: [] }, lastStatus, lastMessage: null,
});
const job = dest('d1', 'Job tasks', 1, 0);
const pers = dest('d2', 'Personal tasks', 2, 1, 'not_shared');
const lookup = { dests: [job, pers], tokenSet: true };

describe('tasks queue', () => {
  test('toReviewCount counts open, keep and sending', () => {
    expect(toReviewCount(['open', 'keep', 'sending', 'sent', 'drop'].map((review) => t({ review: review as TaskRow['review'] })))).toBe(3);
  });
  test('groupQueue orders newest first, null last, deleted meetings named', () => {
    const rows = [
      t({ id: '1', meetingId: 'old', meetingTitle: 'Old', meetingDate: '2026-11-01 09:00:00+00:00' }),
      t({ id: '2', meetingId: null, meetingTitle: 'Gone', meetingDate: '2026-11-05 09:00:00+00:00' }),
      t({ id: '3', meetingId: null, meetingTitle: null, meetingDate: null }),
      t({ id: '4', meetingId: 'new', meetingTitle: 'New', meetingDate: '2026-11-10 09:00:00+00:00' }),
      t({ id: '5', meetingId: 'old', meetingTitle: 'Old', meetingDate: '2026-11-01 09:00:00+00:00' }),
    ];
    const g = groupQueue(rows);
    expect(g.map((x) => x.title)).toEqual(['New', 'Deleted meeting: Gone', 'Old', 'Deleted meeting (title unknown)']);
    expect(g[2].tasks.map((x) => x.id)).toEqual(['1', '5']);
  });
  test('sendCheck follows the order notKept, ambiguous, needDb, token, blocking', () => {
    const keep = (o: Partial<TaskRow>) => t({ review: 'keep', destId: 'd1', ...o });
    expect(sendCheck(t(), lookup).kind).toBe('notKept');
    expect(sendCheck(keep({ sendAmbiguous: true }), lookup).kind).toBe('ambiguous');
    expect(sendCheck(keep({ destId: null }), lookup).kind).toBe('needDb');
    expect(sendCheck(keep({ destId: 'gone' }), lookup).kind).toBe('needDb');
    expect(sendCheck(keep({ destId: 'd2' }), lookup)).toMatchObject({ kind: 'waiting', state: 'not_shared' });
    expect(sendCheck(keep({}), lookup).kind).toBe('sendable');
    expect(sendCheck(keep({}), { dests: [{ ...job, lastStatus: 'offline' }], tokenSet: true }).kind).toBe('sendable');
    expect(sendCheck(keep({}), { ...lookup, tokenSet: false })).toMatchObject({ kind: 'waiting', state: 'token_missing' });
  });
  test('sendSummary counts every kept row but never sends ambiguous ones', () => {
    const rows = [
      t({ id: 'a', review: 'keep', destId: 'd1' }), t({ id: 'b', review: 'keep', destId: 'd1' }),
      t({ id: 'c', review: 'keep', destId: null }), t({ id: 'd', review: 'keep', destId: 'd2' }),
      t({ id: 'e', review: 'keep', destId: 'd1', sendAmbiguous: true }), t({ id: 'f', review: 'open' }), t({ id: 'g', review: 'drop' }),
    ];
    const s = sendSummary(rows, lookup);
    expect(s).toMatchObject({ kept: 5, needDb: 1, waiting: 1, waitingNames: ['Personal tasks'], undecided: 1, sendableIds: ['a', 'b'] });
    expect(s.byDest.map((x) => [x.dest.name, x.n])).toEqual([['Job tasks', 2]]);
  });
  test('copy', () => {
    expect(sendButtonLabel(3, false)).toBe('Send 3 to Notion');
    expect(sendButtonLabel(0, false)).toBe('Nothing to send');
    expect(sendButtonLabel(3, true)).toBe('Sending…');
    expect(resultLine({ sent: 2, failed: 1, ambiguous: 1 })).toEqual({ sent: 'Sent 2 to Notion', notSent: '2 not sent, see To review', aiNote: null });
    expect(resultLine({ sent: 0, failed: 0, ambiguous: 0 })).toEqual({ sent: null, notSent: null, aiNote: null });
    expect(resultLine({ sent: 1, failed: 0, ambiguous: 0, aiNotFilled: ['Hausaufgaben'] }).aiNote).toBe("AI couldn't fill the other columns (Hausaufgaben).");
    expect(resultLine({ sent: 1, failed: 0, ambiguous: 0, aiNotFilled: ['Job tasks: Built-in AI can\'t fill columns, choose a cloud model', 'Personal tasks'] }).aiNote)
      .toBe("AI couldn't fill the other columns (Job tasks: Built-in AI can't fill columns, choose a cloud model; Personal tasks).");
    expect(resultLine({ sent: 1, failed: 0, ambiguous: 0, aiNotFilled: ['Job tasks', 'Job tasks: no cloud model'] }).aiNote)
      .toBe("AI couldn't fill the other columns (Job tasks: no cloud model).");
    expect(waitingText('Personal tasks', 'not_shared')).toBe("Personal tasks isn't shared with Conclave yet, so this task waits.");
    expect(waitingText('Job tasks', 'mapping_broken')).toBe("Job tasks isn't connected (Mapping broken), so this task waits.");
  });
  test('pickFocus keeps, moves to the next, then to the previous', () => {
    expect(pickFocus(['a', 'b', 'c'], ['a', 'b', 'c'], 'b')).toBe('b');
    expect(pickFocus(['a', 'b', 'c'], ['a', 'c'], 'b')).toBe('c');
    expect(pickFocus(['a', 'b', 'c'], ['a', 'b'], 'c')).toBe('b');
    expect(pickFocus(['a', 'b'], ['a'], 'b')).toBe('a');
    expect(pickFocus([], [], 'b')).toBeNull();
  });
  test('destForKey picks by position', () => {
    expect(destForKey('2', [pers, job])?.id).toBe('d2');
    expect(destForKey('1', [pers, job])?.id).toBe('d1');
    expect(destForKey('3', [pers, job])).toBeNull();
    expect(destForKey('0', [pers, job])).toBeNull();
  });
  test('dueLabel', () => {
    expect(dueLabel(t({ dueDate: '2026-11-26' }))).toBe('by Thu 26 Nov');
    expect(dueLabel(t({ due: 'next week' }))).toBe('by next week');
    expect(dueLabel(t())).toBe('no due date');
  });
  test('isEditingText', () => {
    const el = (tag: string, ce?: string) => { const e = document.createElement(tag); if (ce) e.setAttribute('contenteditable', ce); return e; };
    expect([el('input'), el('textarea'), el('div', 'true')].map(isEditingText)).toEqual([true, true, true]);
    expect([el('select'), el('div')].map(isEditingText)).toEqual([false, false]);
  });
});
