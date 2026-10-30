import { describe, expect, test } from 'bun:test';

import type { TemplateJson as Template, TemplateSectionJson as TemplateSection } from '../../src/lib/template-draft';
import { diffItems, matchSections, mergeSelected, collapseSame, wordDiff } from '../../src/lib/templateDiff';

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

const A: TemplateSection = { title: 'Итоги', instruction: 'Подведи итоги.' };
const B: TemplateSection = { title: 'Риски', instruction: 'Перечисли риски.' };
const C: TemplateSection = { title: 'Action items', instruction: 'List actions.' };
const D: TemplateSection = { title: 'Next steps', instruction: 'List next steps.' };
const B2 = { ...B, instruction: B.instruction + ' Укажи владельца.' };
const N = { title: 'English level', instruction: 'Rate English.' };
const old: Template = {
  name: 'Interview', use_when: 'interviews', language: 'ru',
  context: 'Оцени кандидата.', sections: [A, B, C, D], style: 'Bold, not headings.',
};
const next: Template = {
  name: 'Interview', use_when: 'tech interviews', language: 'ru',
  context: 'Оцени кандидата строго.', sections: [D, A, B2, N],
};

describe('matchSections', () => {
  test('matches titles, detects changes and uses the LIS for moves', () => {
    expect(matchSections(old.sections, next.sections)).toEqual([
      { old: 3, next: 0, status: 'moved' },
      { old: 0, next: 1, status: 'unchanged' },
      { old: 1, next: 2, status: 'changed' },
      { next: 3, status: 'added' },
      { old: 2, status: 'removed' },
    ]);
  });

  test('matches positional renames and normalizes title case and whitespace', () => {
    expect(matchSections([A, B], [A, { ...B, title: 'Risks' }])[1]).toEqual({
      old: 1, next: 1, status: 'changed',
    });
    expect(matchSections([{ ...C, title: 'Action  items' }], [{ ...C, title: ' action items' }])[0].old).toBe(0);
  });

  test('matches duplicate titles first unmatched to first unmatched', () => {
    expect(matchSections([A, A, B], [A, B, A]).map(p => [p.old, p.next])).toEqual([
      [0, 0], [2, 1], [1, 2],
    ]);
    expect(matchSections([], [])).toEqual([]);
    expect(matchSections([A], [])).toEqual([{ old: 0, status: 'removed' }]);
    expect(matchSections([], [A])).toEqual([{ next: 0, status: 'added' }]);
  });
});

describe('diffItems', () => {
  test('orders changed items and includes word diffs only where required', () => {
    const items = diffItems(old, next);
    expect(items.map(i => i.key)).toEqual([
      'use_when', 'context', 'section:3:0', 'section:1:2', 'section:-:3', 'section:2:-', 'style',
    ]);
    expect(items.find(i => i.key === 'context')?.parts).toEqual(wordDiff(old.context!, next.context!));
    expect(items.find(i => i.key === 'section:1:2')?.parts).toEqual(
      wordDiff([B.title, B.instruction].join('\n\n'), [B2.title, B2.instruction].join('\n\n')),
    );
    for (const item of items) {
      if (item.kind === 'scalar' || (item.kind === 'section' && item.status !== 'changed')) {
        expect(item).not.toHaveProperty('parts');
      }
    }
    expect(diffItems(old, structuredClone(old))).toEqual([]);
  });

  test('uses rename labels for any string title change', () => {
    for (const title of ['Summary', 'ИТОГИ']) {
      expect(diffItems({ ...old, sections: [A] }, { ...old, sections: [{ ...A, title }] })[0].label)
        .toBe(`Section "Итоги" → "${title}"`);
    }
    expect(diffItems(old, next).find(i => i.key === 'section:-:3')?.label).toBe('Section "English level"');
  });

  test('displays all field labels, kinds and keyword text in the specified order', () => {
    const before: Template = { name: 'Before', description: 'Old', sections: [] };
    const after: Template = {
      name: 'After', description: 'New', use_when: 'Calls', title_keywords: ['one', 'two'],
      language: 'en', context: 'Context', prep: 'Prep', sections: [], style: 'Style',
    };
    const items = diffItems(before, after);
    expect(items.map(i => [i.key, i.label, i.kind])).toEqual([
      ['name', 'Name', 'scalar'], ['description', 'Description', 'text'],
      ['use_when', 'Use when', 'scalar'], ['title_keywords', 'Title keywords', 'scalar'],
      ['language', 'Output language', 'scalar'], ['context', 'Context', 'text'],
      ['prep', 'Prep instruction', 'text'], ['style', 'Style', 'text'],
    ]);
    expect(items.find(i => i.key === 'title_keywords')).toMatchObject({ old: '', next: 'one, two' });
    expect(diffItems(after, before).find(i => i.key === 'style')).toMatchObject({ old: 'Style', next: '' });
  });

  test('section text displays both item formats so neither change is hidden', () => {
    const before = { ...A, example_item_format: 'Legacy' };
    const after = { ...A, item_format: 'Current', example_item_format: 'Legacy' };
    expect(diffItems({ ...old, sections: [before] }, { ...old, sections: [after] })[0]).toMatchObject({
      old: 'Итоги\n\nПодведи итоги.\n\nExample item format: Legacy', next: 'Итоги\n\nПодведи итоги.\n\nCurrent\n\nExample item format: Legacy',
    });
  });
});

describe('mergeSelected', () => {
  test('accepts all or none and deletes selected absent fields', () => {
    const all = new Set(diffItems(old, next).map(i => i.key));
    expect(mergeSelected(old, next, all)).toEqual(next);
    expect(mergeSelected(old, next, all)).not.toHaveProperty('style');
    expect(mergeSelected(old, next, new Set())).toEqual(old);
    const described = { ...old, description: 'Visible deletion' };
    expect(diffItems(described, next).find(i => i.key === 'description')).toMatchObject({ next: '', kind: 'text' });
    expect(mergeSelected(described, next, new Set(['description']))).not.toHaveProperty('description');
  });

  test('applies individual selections at the correct position', () => {
    expect(mergeSelected(old, next, new Set(['context']))).toEqual({ ...old, context: next.context });
    expect(mergeSelected(old, next, new Set(['section:-:3'])).sections).toEqual([A, B, C, D, N]);
    expect(mergeSelected(old, next, new Set(['section:2:-'])).sections).toEqual([A, B, D]);
    expect(mergeSelected(old, next, new Set(['section:3:0'])).sections).toEqual([D, A, B, C]);
  });

  test('anchors changed sections according to the LIS even though their status is changed', () => {
    const before = { ...old, sections: [A, B, C] };
    const changedC = { ...C, instruction: 'Updated actions' };
    const after = { ...old, sections: [changedC, A, B] };
    expect(matchSections(before.sections, after.sections)[0]).toEqual({ old: 2, next: 0, status: 'changed' });
    expect(mergeSelected(before, after, new Set())).toEqual(before);
    expect(mergeSelected(before, after, new Set(['section:2:0']))).toEqual(after);
  });

  test('preserves omitted hidden fields on selected matched sections', () => {
    const before = { ...A, format: 'list', example_item_format: '- example' };
    const after = { ...A, instruction: 'Updated' };
    expect(mergeSelected({ ...old, sections: [before] }, { ...old, sections: [after] }, new Set(['section:0:0'])).sections)
      .toEqual([{ ...after, format: 'list', example_item_format: '- example' }]);
    const explicit = { ...after, format: 'paragraph', example_item_format: 'New example' };
    const target = { ...old, sections: [explicit] };
    expect(mergeSelected({ ...old, sections: [before] }, target, new Set(diffItems({ ...old, sections: [before] }, target).map(i => i.key))))
      .toEqual(target);
    const plain = mergeSelected({ ...old, sections: [A] }, { ...old, sections: [after, N] }, new Set(['section:0:0', 'section:-:1']));
    expect(plain.sections).toEqual([after, N]);
    for (const section of plain.sections) {
      expect(section).not.toHaveProperty('format');
      expect(section).not.toHaveProperty('example_item_format');
    }
  });

  test('returns new objects without mutating either input', () => {
    const before = structuredClone(old);
    const after = structuredClone(next);
    const result = mergeSelected(before, after, new Set(['section:1:2']));
    expect(before).toEqual(old);
    expect(after).toEqual(next);
    expect(result).not.toBe(before);
    expect(result.sections).not.toBe(before.sections);
    for (const section of result.sections) {
      expect(before.sections.includes(section)).toBe(false);
      expect(after.sections.includes(section)).toBe(false);
    }
  });
});

test('hidden section fields have visible diffs even when item_format is present', () => {
  const before = { name: 'Notes', sections: [{ title: 'Notes', instruction: 'List', item_format: 'Item', format: 'old format', example_item_format: 'old example' }] };
  const after = { ...before, sections: [{ ...before.sections[0], format: 'new format', example_item_format: 'new example' }] };
  const item = diffItems(before, after)[0];
  expect(item.parts?.filter(p => p.kind === 'del').map(p => p.text).join('')).toContain('old');
  expect(item.next).toContain('new format');
  expect(item.next).toContain('new example');
  expect(mergeSelected(before, after, new Set([item.key]))).toEqual(after);
});
