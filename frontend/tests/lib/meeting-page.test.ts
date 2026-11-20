import { describe, expect, test } from 'bun:test';
import type { TaskRow } from '../../src/lib/tasks';
import {
  audioSummary,
  splitTasks,
  taskKeyAction,
  type TaskKeyAction,
  type TaskKeyEvent,
} from '../../src/lib/meeting-page';

const task = (over: Partial<TaskRow>): TaskRow => ({
  id: 't-1',
  meetingId: 'm1',
  meetingTitle: 'Meeting 1',
  meetingDate: '2026-11-17',
  text: 'Do something',
  owner: 'me',
  due: null,
  at: null,
  status: 'open',
  source: 'auto',
  createdAt: '2026-11-17T10:00:00Z',
  review: 'open', removedBy: null, destId: null, dueDate: null, notionUrl: null, sentAt: null, sendError: null, sendAmbiguous: false, templateId: null,
  ...over,
});

describe('splitTasks', () => {
  test('empty list', () => {
    expect(splitTasks([], 'm1')).toEqual({ kept: [], removed: [] });
  });

  test('ignores other meetings and unattached tasks', () => {
    const tasks = [task({ id: '1' }), task({ id: '2', meetingId: 'm2' }), task({ id: '3', meetingId: null })];
    const { kept, removed } = splitTasks(tasks, 'm1');
    expect(kept.map((t) => t.id)).toEqual(['1']);
    expect(removed).toEqual([]);
  });

  test('drop is removed whatever the owner; every other review is kept', () => {
    const tasks = [
      task({ id: 'a', review: 'drop', owner: 'me', removedBy: 'user' }),
      task({ id: 'b', review: 'drop', owner: 'unassigned', removedBy: 'ai' }),
      task({ id: 'c', review: 'open', owner: 'unassigned' }),
      task({ id: 'd', review: 'keep' }),
      task({ id: 'e', review: 'sending' }),
      task({ id: 'f', review: 'sent' }),
    ];
    const { kept, removed } = splitTasks(tasks, 'm1');
    expect(removed.map((t) => t.id)).toEqual(['a', 'b']);
    expect(kept.map((t) => t.id)).toEqual(['c', 'd', 'e', 'f']);
  });
});

describe('taskKeyAction', () => {
  test('maps k and K to keep', () => {
    expect(taskKeyAction({ key: 'k' })).toBe('keep');
    expect(taskKeyAction({ key: 'K' })).toBe('keep');
    expect(taskKeyAction({ key: 'K', shiftKey: true })).toBe('keep');
  });

  test('maps x and X to remove', () => {
    expect(taskKeyAction({ key: 'x' })).toBe('remove');
    expect(taskKeyAction({ key: 'X' })).toBe('remove');
    expect(taskKeyAction({ key: 'X', shiftKey: true })).toBe('remove');
  });

  test('maps ArrowUp to up', () => {
    expect(taskKeyAction({ key: 'ArrowUp' })).toBe('up');
  });

  test('maps ArrowDown to down', () => {
    expect(taskKeyAction({ key: 'ArrowDown' })).toBe('down');
  });

  test('returns null when modifier keys are pressed (ctrl, meta, alt)', () => {
    expect(taskKeyAction({ key: 'k', ctrlKey: true })).toBeNull();
    expect(taskKeyAction({ key: 'k', metaKey: true })).toBeNull();
    expect(taskKeyAction({ key: 'k', altKey: true })).toBeNull();
    expect(taskKeyAction({ key: 'K', ctrlKey: true })).toBeNull();
    expect(taskKeyAction({ key: 'x', ctrlKey: true })).toBeNull();
    expect(taskKeyAction({ key: 'x', metaKey: true })).toBeNull();
    expect(taskKeyAction({ key: 'x', altKey: true })).toBeNull();
    expect(taskKeyAction({ key: 'X', ctrlKey: true })).toBeNull();
    expect(taskKeyAction({ key: 'ArrowUp', ctrlKey: true })).toBeNull();
    expect(taskKeyAction({ key: 'ArrowUp', altKey: true })).toBeNull();
    expect(taskKeyAction({ key: 'ArrowDown', ctrlKey: true })).toBeNull();
    expect(taskKeyAction({ key: 'ArrowDown', metaKey: true })).toBeNull();
    expect(taskKeyAction({ key: 'ArrowUp', shiftKey: true })).toBeNull();
    expect(taskKeyAction({ key: 'ArrowDown', shiftKey: true })).toBeNull();
  });

  test('returns null for unmapped keys', () => {
    expect(taskKeyAction({ key: 'Enter' })).toBeNull();
    expect(taskKeyAction({ key: 'Escape' })).toBeNull();
    expect(taskKeyAction({ key: ' ' })).toBeNull();
    expect(taskKeyAction({ key: 'Tab' })).toBeNull();
    expect(taskKeyAction({ key: 'j' })).toBeNull();
    expect(taskKeyAction({ key: 'a' })).toBeNull();
  });
});

describe('audioSummary', () => {
  test('drops the device suffix and lists the allowed languages for Auto', () => {
    expect(audioSummary({ micDevice: 'Headset (input)', systemDevice: 'Speakers (output)', language: 'auto', allowed: ['ru', 'en', 'de'], provider: 'localWhisper' }))
      .toBe('Mic: Headset · System audio: Speakers · Language: Auto (ru, en, de)');
  });
  test('Default devices and a bare Auto for parakeet or an empty list', () => {
    expect(audioSummary({ micDevice: null, systemDevice: null, language: 'auto', allowed: [], provider: 'parakeet' }))
      .toBe('Mic: Default · System audio: Default · Language: Auto');
    expect(audioSummary({ micDevice: null, systemDevice: null, language: 'auto', allowed: ['ru'], provider: 'parakeet' })).toContain('Language: Auto');
    expect(audioSummary({ micDevice: null, systemDevice: null, language: 'auto', allowed: ['ru'], provider: 'parakeet' })).not.toContain('(ru)');
  });
  test('a named language and translation', () => {
    expect(audioSummary({ micDevice: null, systemDevice: null, language: 'de', allowed: [], provider: 'localWhisper' })).toContain('Language: German');
    expect(audioSummary({ micDevice: null, systemDevice: null, language: 'auto-translate', allowed: [], provider: 'localWhisper' })).toContain('Auto, translated to English');
  });
});
