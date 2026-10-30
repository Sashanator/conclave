import { describe, expect, test } from 'bun:test';

import { collapseSame, wordDiff } from '../../src/lib/templateDiff';

describe('wordDiff', () => {
  test('marks a changed word as del+add around unchanged words', () => {
    expect(wordDiff('a b c', 'a x c')).toEqual([
      { kind: 'same', text: 'a ' },
      { kind: 'del', text: 'b ' },
      { kind: 'add', text: 'x ' },
      { kind: 'same', text: 'c' },
    ]);
  });

  test('handles Cyrillic words', () => {
    expect(wordDiff('Оцени уровень английского', 'Оцени уровень немецкого')).toEqual([
      { kind: 'same', text: 'Оцени уровень ' },
      { kind: 'del', text: 'английского' },
      { kind: 'add', text: 'немецкого' },
    ]);
  });

  test('keeps the invariants on a multi-line pair with a leading space', () => {
    const a = ' x\ny z\n- one\n- two';
    const b = ' x\ny Z\n- one\n- three\n- four';
    const parts = wordDiff(a, b);
    const join = (kinds: Array<'same' | 'add' | 'del'>) =>
      parts
        .filter((p) => kinds.includes(p.kind))
        .map((p) => p.text)
        .join('');
    expect(join(['same', 'del'])).toBe(a);
    expect(join(['same', 'add'])).toBe(b);
    for (let i = 1; i < parts.length; i++) {
      expect(parts[i].kind).not.toBe(parts[i - 1].kind);
    }
  });

  test('edge cases: identical, empty old, both empty', () => {
    expect(wordDiff('s', 's')).toEqual([{ kind: 'same', text: 's' }]);
    expect(wordDiff('', 'b')).toEqual([{ kind: 'add', text: 'b' }]);
    expect(wordDiff('', '')).toEqual([]);
  });
});

describe('collapseSame', () => {
  test('replaces the middle of a long run with a marker', () => {
    expect(collapseSame('1\n2\n3\n4\n5\n6\n7')).toBe('1\n2\n… 3 unchanged lines …\n6\n7');
  });

  test('returns short text unchanged', () => {
    expect(collapseSame('1\n2\n3')).toBe('1\n2\n3');
  });
});
