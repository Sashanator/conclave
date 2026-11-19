import { describe, expect, test } from 'bun:test';
import { pickTemplateIfAllowed, REGENERATE_DISCARD_PROMPT, shouldRegenerate } from '../../src/lib/regenerate-guard';

describe('shouldRegenerate', () => {
  test('regenerates without asking when the summary has no unsaved edits', async () => {
    const ask = async () => {
      throw new Error('should not ask');
    };
    expect(await shouldRegenerate(false, ask)).toBe(true);
  });

  test('asks before replacing unsaved edits and regenerates on Yes', async () => {
    const asked: string[] = [];
    expect(await shouldRegenerate(true, async (message) => (asked.push(message), true))).toBe(true);
    expect(asked).toEqual([REGENERATE_DISCARD_PROMPT]);
  });

  test('does not regenerate on No', async () => {
    expect(await shouldRegenerate(true, async () => false)).toBe(false);
  });

  test('returns a Promise, never a truthy value before the answer', () => {
    expect(shouldRegenerate(true, async () => false)).toBeInstanceOf(Promise);
  });

  test('uses the exact confirmation text', () => {
    expect(REGENERATE_DISCARD_PROMPT).toBe('Regenerating replaces your unsaved edits to these notes. Continue?');
  });
});

describe('pickTemplateIfAllowed', () => {
  test('picks and regenerates without asking when there are no unsaved edits', async () => {
    const calls: string[] = [];
    await pickTemplateIfAllowed(false, () => calls.push('pick'), () => calls.push('regenerate'), async () => {
      throw new Error('should not ask');
    });
    expect(calls).toEqual(['pick', 'regenerate']);
  });

  test('asks first; on Yes it picks, then regenerates', async () => {
    const calls: string[] = [];
    await pickTemplateIfAllowed(true, () => calls.push('pick'), () => calls.push('regenerate'), async (message) => {
      calls.push(`ask:${message}`);
      return true;
    });
    expect(calls).toEqual([`ask:${REGENERATE_DISCARD_PROMPT}`, 'pick', 'regenerate']);
  });

  test('on No it neither picks nor regenerates', async () => {
    const calls: string[] = [];
    await pickTemplateIfAllowed(true, () => calls.push('pick'), () => calls.push('regenerate'), async () => false);
    expect(calls).toEqual([]);
  });
});
