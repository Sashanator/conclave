import { describe, expect, test } from 'bun:test';
import { hitTarget, meetingHref, parseMeetingTarget, type SearchHit } from '../../src/lib/meeting-target';

describe('meeting targets', () => {
  test('meetingHref_roundtrip: an id containing & survives the URL', () => {
    const t = { id: 'meeting-a&b=c', line: 'transcript-9', tab: 'notes' as const };
    expect(parseMeetingTarget(new URLSearchParams(meetingHref(t).split('?')[1]))).toEqual(t);
  });

  test('meetingHref writes only the parts that are set', () => {
    expect(meetingHref({ id: 'm1' })).toBe('/meeting-details?id=m1');
    expect(meetingHref({ id: 'm1', line: 't9' })).toBe('/meeting-details?id=m1&line=t9');
  });

  test('parseMeetingTarget rejects a missing id and drops unknown tabs', () => {
    expect(parseMeetingTarget(new URLSearchParams('line=t9'))).toBeNull();
    expect(parseMeetingTarget(new URLSearchParams('id=m1&tab=evil'))).toEqual({ id: 'm1' });
  });

  test('hitTarget maps each source to its target', () => {
    const base = { meetingId: 'm1', title: 'T', snippet: 'x' };
    const transcript: SearchHit = { ...base, source: 'transcript', transcriptId: 't9', audioStart: 4 };
    const summary: SearchHit = { ...base, source: 'summary', transcriptId: null, audioStart: null };
    const notes: SearchHit = { ...base, source: 'notes', transcriptId: null, audioStart: null };
    expect(hitTarget(transcript)).toEqual({ id: 'm1', line: 't9' });
    expect(hitTarget(summary)).toEqual({ id: 'm1', tab: 'summary' });
    expect(hitTarget(notes)).toEqual({ id: 'm1', tab: 'notes' });
  });
});
