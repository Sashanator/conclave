import { describe, expect, test } from 'bun:test';
import { hasVisibleSummaryContent, parseSummaryContent, readSummaryMetadata } from '../../src/lib/summary-content';

describe('summary content validation', () => {
  test('accepts visible markdown and rejects whitespace or reasoning markers', () => {
    expect(hasVisibleSummaryContent({ markdown: '# Summary\nVisible' })).toBe(true);
    expect(hasVisibleSummaryContent({ markdown: '   ' })).toBe(false);
    expect(hasVisibleSummaryContent({ markdown: '<think>private</think>' })).toBe(false);
    expect(hasVisibleSummaryContent({
      markdown: '<thinking class="internal">private</thinking>',
    })).toBe(false);
    expect(hasVisibleSummaryContent({ markdown: '<thinker>Visible</thinker>' })).toBe(true);
  });

  test('validates BlockNote and legacy summary content', () => {
    expect(hasVisibleSummaryContent({ summary_json: [] })).toBe(false);
    expect(hasVisibleSummaryContent({
      summary_json: [{ content: [{ type: 'text', text: 'Visible' }] }],
    })).toBe(true);
    expect(hasVisibleSummaryContent({
      Decisions: { blocks: [{ content: '   ' }] },
    })).toBe(false);
    expect(hasVisibleSummaryContent({
      Decisions: { blocks: [{ content: 'Ship it' }] },
    })).toBe(true);
  });

  test('parses one historical double-encoded payload', () => {
    expect(parseSummaryContent('{"markdown":"Visible"}')).toEqual({ markdown: 'Visible' });
    expect(parseSummaryContent('{not json')).toBeNull();
  });

  test('readSummaryMetadata reads template and language fields', () => {
    const m = readSummaryMetadata({
      markdown: 'x', template_id: 'one_to_one', template_name: 'One-to-One',
      template_source: 'auto', summary_language: 'ru', language_fallback: false,
    });
    expect(m).toMatchObject({ templateId: 'one_to_one', templateName: 'One-to-One', templateSource: 'auto', summaryLanguage: 'ru', languageFallback: false });
  });

  test('readSummaryMetadata tolerates old results', () => {
    const m = readSummaryMetadata({ markdown: 'x', normalization_fallback: true });
    expect(m).toMatchObject({ templateId: null, templateSource: null, languageFallback: false });
  });
});
