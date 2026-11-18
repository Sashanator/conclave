import type { TaskRow } from './tasks';

export type TaskKeyAction = 'keep' | 'remove' | 'up' | 'down';

export type TaskKeyEvent = {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
};

/**
 * Splits tasks for a specific meeting into kept vs removed:
 * - this meeting only (ignores tasks belonging to other meetings or unattached tasks)
 * - removed = owner 'unassigned' or status 'dismissed'
 * - kept = all other tasks for this meeting
 */
export function splitTasks(
  tasks: TaskRow[],
  meetingId: string
): { kept: TaskRow[]; removed: TaskRow[] } {
  const kept: TaskRow[] = [];
  const removed: TaskRow[] = [];
  for (const task of tasks) {
    if (task.meetingId !== meetingId) continue;
    if (task.owner === 'unassigned' || task.status === 'dismissed') {
      removed.push(task);
    } else {
      kept.push(task);
    }
  }
  return { kept, removed };
}

/**
 * Determines task review action from keyboard input:
 * - k / K -> 'keep'
 * - x / X -> 'remove'
 * - ArrowUp / Up -> 'up'
 * - ArrowDown / Down -> 'down'
 * - returns null if a modifier is pressed (ctrl, meta, alt, or shift on non-letter keys)
 * - returns null for unmapped keys
 */
export function taskKeyAction(e: TaskKeyEvent): TaskKeyAction | null {
  if (e.ctrlKey || e.metaKey || e.altKey) {
    return null;
  }
  if (e.shiftKey && e.key.toLowerCase() !== 'k' && e.key.toLowerCase() !== 'x') {
    return null;
  }
  switch (e.key) {
    case 'k':
    case 'K':
      return 'keep';
    case 'x':
    case 'X':
      return 'remove';
    case 'ArrowUp':
    case 'Up':
      return 'up';
    case 'ArrowDown':
    case 'Down':
      return 'down';
    default:
      return null;
  }
}
