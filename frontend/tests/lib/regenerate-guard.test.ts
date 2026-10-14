import { describe, expect, test } from 'bun:test';
import { pickTemplateIfAllowed, REGENERATE_DISCARD_PROMPT, shouldRegenerate } from '../../src/lib/regenerate-guard';

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

describe('pickTemplateIfAllowed', () => {
  test('picks and regenerates without asking when there are no unsaved edits', () => {
    const calls: string[] = [];
    pickTemplateIfAllowed(false, () => calls.push('pick'), () => calls.push('regenerate'), () => {
      throw new Error('should not ask');
    });
    expect(calls).toEqual(['pick', 'regenerate']);
  });

  test('asks first; on Yes it picks, then regenerates', () => {
    const calls: string[] = [];
    pickTemplateIfAllowed(true, () => calls.push('pick'), () => calls.push('regenerate'), (message) => {
      calls.push(`ask:${message}`);
      return true;
    });
    expect(calls).toEqual([`ask:${REGENERATE_DISCARD_PROMPT}`, 'pick', 'regenerate']);
  });

  test('on No it neither picks nor regenerates', () => {
    const calls: string[] = [];
    pickTemplateIfAllowed(true, () => calls.push('pick'), () => calls.push('regenerate'), () => false);
    expect(calls).toEqual([]);
  });
});
