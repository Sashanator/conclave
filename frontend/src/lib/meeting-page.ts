import { LANGUAGES } from '../constants/languages';
import type { TaskRow } from './tasks';

export type TaskKeyAction = 'keep' | 'remove' | 'up' | 'down';

export type TaskKeyEvent = {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
};

/** This meeting's tasks by review state: removed = `drop`, kept = every other review (open, keep, sending, sent). */
export function splitTasks(
  tasks: TaskRow[],
  meetingId: string
): { kept: TaskRow[]; removed: TaskRow[] } {
  const mine = tasks.filter((t) => t.meetingId === meetingId);
  return { kept: mine.filter((t) => t.review !== 'drop'), removed: mine.filter((t) => t.review === 'drop') };
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

/** `Mic: Headset · System audio: Speakers · Language: Auto (ru, en, de)` for the next recording. */
export function audioSummary(o: { micDevice: string | null; systemDevice: string | null; language: string; allowed: string[]; provider: string }): string {
  const device = (d: string | null) => (d ? d.replace(/ \((input|output)\)$/, '') : 'Default');
  const auto = o.allowed.length > 0 && o.provider !== 'parakeet' ? `Auto (${o.allowed.join(', ')})` : 'Auto';
  const language = o.language === 'auto' ? auto : o.language === 'auto-translate' ? 'Auto, translated to English' : (LANGUAGES.find((l) => l.code === o.language)?.name ?? o.language);
  return `Mic: ${device(o.micDevice)} · System audio: ${device(o.systemDevice)} · Language: ${language}`;
}
