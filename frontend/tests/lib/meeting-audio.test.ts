import { describe, expect, test } from 'bun:test';
import { findActiveSegmentId } from '../../src/lib/meeting-audio';

const segs = [
  { id: 'a', timestamp: 0, endTime: 4, text: 'a' },
  { id: 'b', timestamp: 5, text: 'b' },
  { id: 'c', timestamp: 9, endTime: 12, text: 'c' },
];

describe('findActiveSegmentId', () => {
  test('finds the segment under the playhead', () => {
    expect(findActiveSegmentId(segs, 1)).toBe('a');
    expect(findActiveSegmentId(segs, 6)).toBe('b'); // no endTime: until the next start
    expect(findActiveSegmentId(segs, 10)).toBe('c');
  });
  test('gaps, before start, after end and no playhead', () => {
    expect(findActiveSegmentId(segs, 4.5)).toBeNull();
    expect(findActiveSegmentId(segs, 13)).toBeNull();
    expect(findActiveSegmentId(segs, null)).toBeNull();
    expect(findActiveSegmentId([], 1)).toBeNull();
  });
});
