import { beforeEach, describe, expect, test } from 'bun:test';
import { readStoredRatio, writeStoredRatio } from '../../src/lib/meeting-page-layout';

beforeEach(() => localStorage.clear());

describe('meeting page layout', () => {
  test('ratios are stored per key and clamped to their range', () => {
    writeStoredRatio('conclave.threeCol.transcriptRatio', 0.4);
    writeStoredRatio('meetily.meetingDetails.transcriptPaneRatio', 0.45);
    expect(readStoredRatio('conclave.threeCol.transcriptRatio', 0.3, 0.2, 0.5)).toBe(0.4);
    expect(readStoredRatio('meetily.meetingDetails.transcriptPaneRatio', 0.3, 0.3, 0.5)).toBe(0.45);
    writeStoredRatio('k', 0.9);
    expect(readStoredRatio('k', 0.3, 0.2, 0.5)).toBe(0.3);
    expect(readStoredRatio('missing', 0.3, 0.2, 0.5)).toBe(0.3);
  });
});
