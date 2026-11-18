import { describe, expect, test } from 'bun:test';
import {
  clock,
  groupHits,
  hitTarget,
  lineAtTime,
  meetingHref,
  parseMeetingTarget,
  snippetHref,
  snippetLabel,
  type SearchHit,
} from '../../src/lib/meeting-target';

describe('meeting targets', () => {
  test('meetingHref_roundtrip: an id containing & survives the URL', () => {
    const t = { id: 'meeting-a&b=c', line: 'transcript-9', tab: 'notes' as const };
    expect(parseMeetingTarget(new URLSearchParams(meetingHref(t).split('?')[1]))).toEqual(t);
  });

  test('meetingHref_roundtrip_time: a time in seconds survives the URL', () => {
    const t = { id: 'm1', t: 62.5 };
    expect(meetingHref(t)).toBe('/meeting-details?id=m1&t=62.5');
    expect(parseMeetingTarget(new URLSearchParams(meetingHref(t).split('?')[1]))).toEqual(t);
  });

  test('parseMeetingTarget drops a time that is not a non-negative number', () => {
    expect(parseMeetingTarget(new URLSearchParams('id=m1&t=abc'))).toEqual({ id: 'm1' });
    expect(parseMeetingTarget(new URLSearchParams('id=m1&t=-4'))).toEqual({ id: 'm1' });
  });

  test('lineAtTime picks the last row at or before the time', () => {
    const rows = [
      { id: 'a', timestamp: 0 },
      { id: 'b', timestamp: 60 },
      { id: 'c', timestamp: 120 },
    ];
    expect(lineAtTime(rows, 61, false)).toBe('b');
    expect(lineAtTime(rows, 60, true)).toBe('b');
    expect(lineAtTime(rows, -5, false)).toBe('a');
    expect(lineAtTime(rows, 200, false)).toBe('c');
  });

  test('lineAtTime waits while the row for the time may be on a page not loaded yet', () => {
    const rows = [{ id: 'a', timestamp: 0 }, { id: 'b', timestamp: 60 }];
    expect(lineAtTime(rows, 200, true)).toBeNull();
    expect(lineAtTime([], 5, false)).toBeNull();
  });

  test('meetingHref writes only the parts that are set', () => {
    expect(meetingHref({ id: 'm1' })).toBe('/meeting-details?id=m1');
    expect(meetingHref({ id: 'm1', line: 't9' })).toBe('/meeting-details?id=m1&line=t9');
  });

  test('parseMeetingTarget rejects a missing id and drops unknown tabs', () => {
    expect(parseMeetingTarget(new URLSearchParams('line=t9'))).toBeNull();
    expect(parseMeetingTarget(new URLSearchParams('id=m1&tab=evil'))).toEqual({ id: 'm1' });
  });

  test('parseMeetingTarget accepts prep tab', () => {
    expect(parseMeetingTarget(new URLSearchParams('id=a&tab=prep'))?.tab).toBe('prep');
  });

  test('hitTarget maps each source to its target', () => {
    const base = { meetingId: 'm1', title: 'T', snippet: 'x', totalInMeeting: 1 };
    const transcript: SearchHit = { ...base, source: 'transcript', transcriptId: 't9', audioStart: 4 };
    const summary: SearchHit = { ...base, source: 'summary', transcriptId: null, audioStart: null };
    const notes: SearchHit = { ...base, source: 'notes', transcriptId: null, audioStart: null };
    expect(hitTarget(transcript)).toEqual({ id: 'm1', line: 't9' });
    expect(hitTarget(summary)).toEqual({ id: 'm1', tab: 'summary' });
    expect(hitTarget(notes)).toEqual({ id: 'm1', tab: 'notes' });
  });

  test('snippetLabel labels hit by source and time', () => {
    const base = { meetingId: 'm1', title: 'T', snippet: 'x', totalInMeeting: 1, transcriptId: null, audioStart: null };
    expect(snippetLabel({ ...base, source: 'title' })).toBe('Name');
    expect(snippetLabel({ ...base, source: 'summary' })).toBe('Summary');
    expect(snippetLabel({ ...base, source: 'notes' })).toBe('Notes');
    expect(snippetLabel({ ...base, source: 'transcript', audioStart: 724 })).toBe('Transcript 12:04');
    expect(snippetLabel({ ...base, source: 'transcript', audioStart: null })).toBe('Transcript');
  });

  test('snippetHref links to time for transcript with audioStart, else hitTarget', () => {
    const base = { meetingId: 'm1', title: 'T', snippet: 'x', totalInMeeting: 1, transcriptId: 't1' };
    expect(snippetHref({ ...base, source: 'transcript', audioStart: 724.6 })).toBe('/meeting-details?id=m1&t=724');
    expect(snippetHref({ ...base, source: 'summary', audioStart: null })).toBe('/meeting-details?id=m1&tab=summary');
    expect(snippetHref({ ...base, source: 'title', audioStart: null })).toBe('/meeting-details?id=m1');
  });

  test('groupHits groups hits by meeting and reports more remaining', () => {
    const m2t: SearchHit = { meetingId: 'm2', title: 'M2', source: 'transcript', snippet: 't', transcriptId: 't1', audioStart: 10, totalInMeeting: 5 };
    const m2s: SearchHit = { meetingId: 'm2', title: 'M2', source: 'summary', snippet: 's', transcriptId: null, audioStart: null, totalInMeeting: 5 };
    const m1t: SearchHit = { meetingId: 'm1', title: 'M1', source: 'transcript', snippet: 't', transcriptId: 't2', audioStart: 20, totalInMeeting: 1 };

    expect(groupHits([m2t, m2s, m1t])).toEqual([
      { meetingId: 'm2', hits: [m2t, m2s], more: 3 },
      { meetingId: 'm1', hits: [m1t], more: 0 },
    ]);
  });
});
