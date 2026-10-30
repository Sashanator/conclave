import { afterAll, describe, expect, mock, test } from 'bun:test';

const originals = { core: { ...(await import('@tauri-apps/api/core')) } };

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
});

const invoke = mock(async (_command: string, _args?: unknown): Promise<unknown> => null);
mock.module('@tauri-apps/api/core', () => ({ invoke }));

const {
  RAW_ANSWER_MARKER, draftTemplate, deletedTemplates, estimateCostUsd, getAssistSettings, historyLabel,
  originFor, reviseTemplate, saveAssistSettings, splitRawAnswer, templateHistory, templateVersion,
} = await import('../../src/lib/templateAssist');
type HistoryEntry = import('../../src/lib/templateAssist').HistoryEntry;
type PendingOrigin = import('../../src/lib/templateAssist').PendingOrigin;
type TemplateDraft = import('../../src/lib/template-draft').TemplateDraft;

const draft: TemplateDraft = {
  name: 'Standup', use_when: '', title_keywords: '', language: 'auto', context: '', prep: '',
  sections: [{ title: 'Done', instruction: 'List done items' }], style: '',
};

describe('template assist invoke wrappers', () => {
  test('reviseTemplate fills the defaults the Rust command expects', async () => {
    invoke.mockClear();
    await reviseTemplate({ templateId: 't', request: 'r' });
    expect(invoke).toHaveBeenCalledWith('api_template_revise', {
      templateId: 't', request: 'r', meetingId: null, includeEdits: false, includeTranscript: false,
      previous: null, refinement: null,
    });
  });

  test('draftTemplate sends the description', async () => {
    invoke.mockClear();
    await draftTemplate('d');
    expect(invoke).toHaveBeenCalledWith('api_template_draft', { description: 'd' });
  });

  test('templateHistory, templateVersion and deletedTemplates use the Rust argument names', async () => {
    invoke.mockClear();
    await templateHistory('t');
    expect(invoke).toHaveBeenLastCalledWith('api_template_history', { templateId: 't' });
    await templateVersion('t', 'v');
    expect(invoke).toHaveBeenLastCalledWith('api_template_version', { templateId: 't', version: 'v' });
    await deletedTemplates();
    expect(invoke).toHaveBeenLastCalledWith('api_deleted_templates');
  });

  test('settings wrappers pass the settings object under settings', async () => {
    invoke.mockClear();
    await getAssistSettings();
    expect(invoke).toHaveBeenLastCalledWith('template_assist_get_settings');
    const s = { provider: null, model: null, includeTranscript: false, hideEditHint: true };
    await saveAssistSettings(s);
    expect(invoke).toHaveBeenLastCalledWith('template_assist_save_settings', { settings: s });
  });
});

describe('historyLabel', () => {
  const entry = (source: HistoryEntry['source'], note = ''): HistoryEntry =>
    ({ version: 'v', savedAt: '2026-10-30T00:00:00Z', source, note });

  test('labels each source', () => {
    expect(historyLabel(entry('ai', 'Added English'))).toBe('Before AI change: Added English');
    expect(historyLabel(entry('manual'))).toBe('Before manual edit');
    expect(historyLabel(entry('restore'))).toBe('Before restore');
    expect(historyLabel(entry('delete'))).toBe('Deleted');
  });
});

describe('splitRawAnswer', () => {
  test('separates the message from the raw model answer', () => {
    expect(splitRawAnswer('Bad template' + RAW_ANSWER_MARKER + '{"x":1}')).toEqual({ message: 'Bad template', raw: '{"x":1}' });
  });

  test('returns no raw answer when the marker is absent', () => {
    expect(splitRawAnswer('timeout')).toEqual({ message: 'timeout', raw: null });
  });
});

describe('estimateCostUsd', () => {
  test('uses chars/4 as tokens and per-million prices', () => {
    expect(estimateCostUsd(4000, 2000, { input: 3, output: 15 })).toBeCloseTo(0.0105, 10);
  });
});

describe('originFor', () => {
  const pending = (d: TemplateDraft, source: 'ai' | 'manual' = 'ai'): PendingOrigin =>
    ({ json: JSON.stringify(d), origin: { source, note: 'n' } });

  test('keeps the pending AI origin while the draft is unchanged', () => {
    expect(originFor(draft, pending(draft))).toEqual({ source: 'ai', note: 'n' });
  });

  test('falls back to a manual origin once the draft changed', () => {
    expect(originFor({ ...draft, name: 'x' }, pending(draft))).toEqual({ source: 'manual', note: '' });
  });

  test('is manual without a pending origin', () => {
    expect(originFor(draft, null)).toEqual({ source: 'manual', note: '' });
  });
});
