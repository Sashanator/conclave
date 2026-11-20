import { describe, expect, test } from 'bun:test';
import { previewHandlers as h } from '../../src/lib/browser-preview';

describe('browser preview mocks', () => {
  test('the queue seed covers every review state', () => {
    const rows = h.tasks_queue_list({}) as any[];
    expect(new Set(rows.map((t) => t.review))).toEqual(new Set(['open', 'keep', 'drop', 'sent']));
    expect(rows.some((t) => t.sendAmbiguous)).toBe(true);
    expect(rows.some((t) => t.meetingId === null)).toBe(true);
  });
  test('send fails the unshared destination and sends the rest', () => {
    const rows = h.tasks_queue_list({}) as any[];
    const open = rows.filter((t) => t.review === 'open' && t.destId);
    for (const t of open) h.tasks_set_review({ id: t.id, review: 'keep' });
    expect(h.notion_send({ taskIds: open.map((t) => t.id) })).toEqual({ sent: open.length - 1, failed: 1, ambiguous: 0 });
  });
  test('an empty calls marker is rejected', () => {
    expect(() => h.knowledge_save_settings({ input: { calendar_marker: '' } })).toThrow('The calls marker must be 1–16 characters');
    expect((h.knowledge_save_settings({ input: { calendar_marker: '☎' } }) as any).calendar_marker).toBe('☎');
  });
  test('ask cites preview-1, copilot has the alert word, recording stays idle', () => {
    expect((h.ask_meetings({}) as any).citations[0].meetingId).toBe('preview-1');
    expect((h.copilot_get_settings({}) as any).alert_words).toEqual(['deadline']);
    expect((h.get_recording_state({}) as any).is_recording).toBe(false);
  });
  test('saving a provider key answers null, dictate settings exist', () => {
    expect(h.api_save_provider_key({ provider: 'openai', apiKey: 'k' })).toBeNull();
    expect((h.dictate_get_settings({}) as any).dictionary).toEqual(['Conclave']);
  });
  test('preview-new has no times and preview-1 has the 📝 template', () => {
    const rows = h.api_list_meeting_rows({}) as any[];
    expect(rows.find((r) => r.id === 'preview-new').startedAt).toBeNull();
    expect(rows.find((r) => r.id === 'preview-1')).toMatchObject({ templateId: 'standard_meeting', emoji: '📝', hasSummary: true });
  });
});
