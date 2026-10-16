import { beforeEach, describe, expect, test } from 'bun:test';
import { parseMeetingPageLayout, readMeetingPageLayout, writeMeetingPageLayout, readStoredRatio, writeStoredRatio, readFlag, writeFlag } from '../../src/lib/meeting-page-layout';

beforeEach(() => localStorage.clear());

describe('meeting page layout', () => {
  test('defaults to three columns and rejects junk', () => {
    expect(readMeetingPageLayout()).toBe('three-columns');
    expect(parseMeetingPageLayout('subtab')).toBe('subtab');
    expect(parseMeetingPageLayout('nonsense')).toBe('three-columns');
    writeMeetingPageLayout('subtab');
    expect(readMeetingPageLayout()).toBe('subtab');
  });

  test('ratios are stored per key and clamped to their range', () => {
    writeStoredRatio('conclave.threeCol.transcriptRatio', 0.4);
    writeStoredRatio('meetily.meetingDetails.transcriptPaneRatio', 0.45);
    expect(readStoredRatio('conclave.threeCol.transcriptRatio', 0.3, 0.2, 0.5)).toBe(0.4);
    expect(readStoredRatio('meetily.meetingDetails.transcriptPaneRatio', 0.3, 0.3, 0.5)).toBe(0.45);
    writeStoredRatio('k', 0.9);
    expect(readStoredRatio('k', 0.3, 0.2, 0.5)).toBe(0.3);
    expect(readStoredRatio('missing', 0.3, 0.2, 0.5)).toBe(0.3);
  });

  test('flags', () => {
    expect(readFlag('conclave.notesPaneCollapsed')).toBe(false);
    writeFlag('conclave.notesPaneCollapsed', true);
    expect(readFlag('conclave.notesPaneCollapsed')).toBe(true);
  });
});
