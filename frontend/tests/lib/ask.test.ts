import { describe, expect, test } from 'bun:test';
import { citeLabel, citationTarget, nextAskRequestId, splitAnswer, type Citation } from '../../src/lib/ask';

const transcript: Citation = {
  id: 'E3',
  meetingId: 'm-1',
  meetingTitle: 'Standup',
  date: '2026-10-20',
  transcriptId: 'tx-9',
  audioStart: 724,
  source: 'transcript',
};
const summary: Citation = {
  id: 'E5',
  meetingId: 'm-2',
  meetingTitle: 'Review',
  date: '2026-10-19',
  transcriptId: null,
  audioStart: null,
  source: 'summary',
};

describe('splitAnswer', () => {
  test('a known id becomes a cite part between the text parts', () => {
    expect(splitAnswer('Ship it [E3].', [transcript])).toEqual([
      { kind: 'text', text: 'Ship it ' },
      { kind: 'cite', citation: transcript },
      { kind: 'text', text: '.' },
    ]);
  });

  test('an unknown id is removed and the text around it joins up', () => {
    expect(splitAnswer('Maybe[E9] yes', [transcript])).toEqual([{ kind: 'text', text: 'Maybe yes' }]);
  });

  test('a repeated id gives one cite part per occurrence', () => {
    expect(splitAnswer('A[E3] B[E3]', [transcript])).toEqual([
      { kind: 'text', text: 'A' },
      { kind: 'cite', citation: transcript },
      { kind: 'text', text: ' B' },
      { kind: 'cite', citation: transcript },
    ]);
  });

  test('text without markers is one text part, and empty text gives no parts', () => {
    expect(splitAnswer('Plain', [])).toEqual([{ kind: 'text', text: 'Plain' }]);
    expect(splitAnswer('', [transcript])).toEqual([]);
  });
});

describe('citation links and labels', () => {
  test('a transcript citation opens the transcript at its line', () => {
    expect(citationTarget(transcript)).toEqual({ id: 'm-1', line: 'tx-9' });
  });

  test('summary and notes citations open their own tab', () => {
    expect(citationTarget(summary)).toEqual({ id: 'm-2', tab: 'summary' });
    expect(citationTarget({ ...summary, source: 'notes' })).toEqual({ id: 'm-2', tab: 'notes' });
  });

  test('the label shows title, day and the audio time when there is one', () => {
    expect(citeLabel(transcript)).toBe('Standup · 20 Oct · 12:04');
    expect(citeLabel({ ...transcript, audioStart: 3725 })).toBe('Standup · 20 Oct · 1:02:05');
    expect(citeLabel(summary)).toBe('Review · 19 Oct');
  });
});

describe('nextAskRequestId', () => {
  test('each question gets a larger id', () => {
    const first = nextAskRequestId();
    expect(nextAskRequestId()).toBeGreaterThan(first);
  });
});
