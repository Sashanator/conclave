import { invoke } from '@tauri-apps/api/core';
import type { TemplateDraft, TemplateJson } from '@/lib/template-draft';

export type ChangeNote = { field: string; note: string };
export type AssistResult = { template: TemplateJson; changes: ChangeNote[]; warnings: string[]; base?: TemplateJson | null };
export type HistorySource = 'manual' | 'ai' | 'restore' | 'delete';
export type SaveOrigin = { source: HistorySource; note: string };
export type HistoryEntry = { version: string; savedAt: string; source: HistorySource; note: string };
export type DeletedTemplate = { id: string; name: string; deletedAt: string; version: string };
export type TemplateAssistSettings = { provider: string | null; model: string | null; includeTranscript: boolean; hideEditHint: boolean };
export type PendingOrigin = { json: string; origin: SaveOrigin };   // json = JSON.stringify(draft) when the draft was set

// Rust appends this marker plus the model's raw answer to an "Invalid output after repair" error.
export const RAW_ANSWER_MARKER = '\n\nRaw answer:\n';

export const draftTemplate = (description: string) =>
  invoke<AssistResult>('api_template_draft', { description });

export function reviseTemplate(a: { templateId: string; request: string; previous?: AssistResult | null; refinement?: string | null }): Promise<AssistResult> {
  return invoke<AssistResult>('api_template_revise', {
    templateId: a.templateId, request: a.request, meetingId: null, includeEdits: false, includeTranscript: false,
    previous: a.previous ?? null, refinement: a.refinement ?? null,
  });
}

export const templateHistory = (templateId: string) =>
  invoke<HistoryEntry[]>('api_template_history', { templateId });

export const templateVersion = (templateId: string, version: string) =>
  invoke<TemplateJson>('api_template_version', { templateId, version });

export const deletedTemplates = () => invoke<DeletedTemplate[]>('api_deleted_templates');

export const getAssistSettings = () => invoke<TemplateAssistSettings>('template_assist_get_settings');

export const saveAssistSettings = (settings: TemplateAssistSettings) =>
  invoke<void>('template_assist_save_settings', { settings });

export function historyLabel(e: HistoryEntry): string {
  switch (e.source) {
    case 'ai': return e.note ? `Before AI change: ${e.note}` : 'Before AI change';
    case 'manual': return 'Before manual edit';
    case 'restore': return 'Before restore';
    case 'delete': return 'Deleted';
  }
}

export function splitRawAnswer(err: unknown): { message: string; raw: string | null } {
  const s = String(err);
  const i = s.indexOf(RAW_ANSWER_MARKER);
  return i < 0
    ? { message: s, raw: null }
    : { message: s.slice(0, i), raw: s.slice(i + RAW_ANSWER_MARKER.length) };
}

// Rough estimate: 1 token ~ 4 characters. Prices are $ per million tokens.
export function estimateCostUsd(inputChars: number, outputChars: number, price: { input: number; output: number }): number {
  return (inputChars / 4) * price.input / 1_000_000 + (outputChars / 4) * price.output / 1_000_000;
}

// The pending AI origin applies only while the draft still equals the JSON it was recorded with.
export function originFor(draft: TemplateDraft, pending: PendingOrigin | null): SaveOrigin {
  return pending && pending.json === JSON.stringify(draft) ? pending.origin : { source: 'manual', note: '' };
}
