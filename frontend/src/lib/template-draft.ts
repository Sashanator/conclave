export interface TemplateSectionJson { title: string; instruction: string; format?: string; item_format?: string; example_item_format?: string }
export interface TemplateJson {
  name: string; description?: string; use_when?: string; title_keywords?: string[]; language?: string;
  context?: string; prep?: string; sections: TemplateSectionJson[]; style?: string;
}
export interface TemplateDraft {
  name: string; use_when: string; title_keywords: string; language: string; context: string; prep: string;
  sections: Array<{ title: string; instruction: string; item_format?: string }>; style: string;
}

export const emptyDraft = (): TemplateDraft => ({
  name: '', use_when: '', title_keywords: '', language: 'auto', context: '', prep: '', sections: [{ title: '', instruction: '' }], style: '',
});

export function draftFromTemplate(t: TemplateJson): TemplateDraft {
  return {
    name: t.name, use_when: t.use_when ?? '', title_keywords: (t.title_keywords ?? []).join(', '),
    language: t.language || 'auto', context: t.context ?? '', prep: t.prep ?? '', style: t.style ?? '',
    sections: t.sections.map(s => ({
      title: s.title, instruction: s.instruction,
      ...(s.item_format ?? s.example_item_format ? { item_format: s.item_format ?? s.example_item_format } : {}),
    })),
  };
}

export function templateFromDraft(d: TemplateDraft): TemplateJson {
  const t: TemplateJson = { name: d.name.trim(), sections: d.sections.map(s => ({
    title: s.title.trim(), instruction: s.instruction.trim(), ...(s.item_format?.trim() ? { item_format: s.item_format.trim() } : {}),
  })) };
  const keywords = d.title_keywords.split(',').map(k => k.trim()).filter(Boolean);
  if (d.use_when.trim()) t.use_when = d.use_when.trim();
  if (keywords.length) t.title_keywords = keywords;
  if (d.language && d.language !== 'auto') t.language = d.language;
  if (d.context.trim()) t.context = d.context.trim();
  if (d.prep.trim()) t.prep = d.prep.trim();
  if (d.style.trim()) t.style = d.style.trim();
  return t;
}

export function validateDraft(d: TemplateDraft): string | null {
  if (!d.name.trim()) return 'Name is required';
  if (d.sections.length === 0) return 'Add at least one section';
  for (const [i, s] of d.sections.entries()) {
    if (!s.title.trim()) return `Section ${i + 1} needs a title`;
    if (!s.instruction.trim()) return `Section "${s.title.trim()}" needs an instruction`;
  }
  return null;
}
