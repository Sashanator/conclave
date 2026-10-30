import { describe, expect, test } from 'bun:test';
import { draftFromTemplate, emptyDraft, templateFromDraft, validateDraft } from '@/lib/template-draft';

describe('template drafts', () => {
  test('round trip keeps hidden fields', () => {
    const t = { name: 'N', description: 'D', sections: [{ title: 'A', instruction: 'i', format: 'list', example_item_format: '- x' }] };
    const draft = draftFromTemplate(t);
    expect(templateFromDraft(draft)).toEqual(t);
    expect(JSON.stringify(draft)).toBe(JSON.stringify(draftFromTemplate(t)));
    expect(draft.sections[0].item_format ?? draft.sections[0].example_item_format).toBe('- x');
  });

  test('item_format edited in the editor wins', () => {
    const t = { name: 'N', description: 'D', sections: [{ title: 'A', instruction: 'i', format: 'list', example_item_format: '- x' }] };
    const draft = draftFromTemplate(t);
    draft.sections[0].item_format = '- y';
    expect(templateFromDraft(draft).sections[0]).toEqual({ title: 'A', instruction: 'i', format: 'list', item_format: '- y' });
  });

  test('new section has no hidden fields', () => {
    const t = { name: 'N', description: 'D', sections: [{ title: 'A', instruction: 'i', format: 'list', example_item_format: '- x' }] };
    const draft = draftFromTemplate(t);
    draft.sections.push({ title: 'B', instruction: 'j' });
    expect(templateFromDraft(draft).sections[1]).toEqual({ title: 'B', instruction: 'j' });
  });

  test('unchanged fallback stays a legacy example and clearing it removes it', () => {
    const t = { name: 'N', sections: [{ title: 'A', instruction: 'i', example_item_format: '- x' }] };
    const draft = draftFromTemplate(t);
    draft.sections[0].item_format = '- x';
    expect(templateFromDraft(draft)).toEqual(t);
    draft.sections[0].item_format = '';
    expect(templateFromDraft(draft).sections[0]).toEqual({ title: 'A', instruction: 'i' });
  });

  test('hidden fields retain empty strings and whitespace', () => {
    const t = { name: 'N', description: ' D ', sections: [{ title: 'A', instruction: 'i', format: '', example_item_format: ' - x ' }] };
    expect(templateFromDraft(draftFromTemplate(t))).toEqual(t);
    expect(templateFromDraft(draftFromTemplate({ ...t, description: '' })).description).toBe('');
  });

  test('round trip keeps every field and drops blanks', () => {
    const t = { name: 'X', use_when: 'u', title_keywords: ['a', 'b'], language: 'ru', context: 'c', prep: 'p',
      sections: [{ title: 'S', instruction: 'I', item_format: '| A |' }], style: 's' };
    expect(templateFromDraft(draftFromTemplate(t))).toEqual(t);
    expect(emptyDraft().prep).toBe('');
    expect(templateFromDraft({ ...emptyDraft(), name: 'M', prep: '  ', sections: [{ title: 'S', instruction: 'I' }] })).not.toHaveProperty('prep');
    const minimal = templateFromDraft({ ...emptyDraft(), name: 'M', sections: [{ title: 'S', instruction: 'I' }] });
    expect(minimal).toEqual({ name: 'M', sections: [{ title: 'S', instruction: 'I' }] });
  });

  test('keywords are split, trimmed and blank ones dropped', () => {
    const d = { ...emptyDraft(), name: 'M', title_keywords: ' 1-2-1 , ,ЛПР ', sections: [{ title: 'S', instruction: 'I' }] };
    expect(templateFromDraft(d).title_keywords).toEqual(['1-2-1', 'ЛПР']);
  });

  test('validation messages', () => {
    const ok = { ...emptyDraft(), name: 'N', sections: [{ title: 'S', instruction: 'I' }] };
    expect(validateDraft(ok)).toBeNull();
    expect(validateDraft({ ...ok, name: ' ' })).toBe('Name is required');
    expect(validateDraft({ ...ok, sections: [] })).toBe('Add at least one section');
    expect(validateDraft({ ...ok, sections: [{ title: ' ', instruction: 'I' }] })).toBe('Section 1 needs a title');
    expect(validateDraft({ ...ok, sections: [{ title: 'S', instruction: '' }] })).toBe('Section "S" needs an instruction');
  });

  test('a Meetily-era template without new fields loads', () => {
    const d = draftFromTemplate({ name: 'Old', description: 'd', sections: [{ title: 'A', instruction: 'B', format: 'list' }] });
    expect(d.language).toBe('auto');
    expect(d.title_keywords).toBe('');
  });
});
