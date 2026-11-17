import { describe, expect, test } from 'bun:test';
import { previewHandlers as h } from '../../src/lib/browser-preview';

describe('browser preview mocks', () => {
  test('preview-1 has three tasks: me, unassigned, dismissed', () => {
    const all = ['open', 'done', 'dismissed'].flatMap((status) => h.tasks_list({ status }) as any[]).filter((t) => t.meetingId === 'preview-1');
    expect(all.map((t) => `${t.owner}/${t.status}`).sort()).toEqual(['me/open', 'unassigned/dismissed', 'unassigned/open']);
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
  test('preview-new has no times and preview-1 has the 📝 template', () => {
    const rows = h.api_list_meeting_rows({}) as any[];
    expect(rows.find((r) => r.id === 'preview-new').startedAt).toBeNull();
    expect(rows.find((r) => r.id === 'preview-1')).toMatchObject({ templateId: 'standard_meeting', emoji: '📝', hasSummary: true });
  });
});
