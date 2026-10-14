import { describe, expect, test } from 'bun:test';
import { REGENERATE_DISCARD_PROMPT, shouldRegenerate } from '../../src/lib/regenerate-guard';

describe('shouldRegenerate', () => {
  test('regenerates without asking when the summary has no unsaved edits', () => {
    const ask = () => {
      throw new Error('should not ask');
    };
    expect(shouldRegenerate(false, ask)).toBe(true);
  });

  test('asks before replacing unsaved edits and regenerates on Yes', () => {
    const asked: string[] = [];
    expect(shouldRegenerate(true, (message) => (asked.push(message), true))).toBe(true);
    expect(asked).toEqual([REGENERATE_DISCARD_PROMPT]);
  });

  test('does not regenerate on No', () => {
    expect(shouldRegenerate(true, () => false)).toBe(false);
  });

  test('uses the exact confirmation text', () => {
    expect(REGENERATE_DISCARD_PROMPT).toBe('Regenerating replaces your unsaved edits to these notes. Continue?');
  });
});
