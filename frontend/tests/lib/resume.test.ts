import { describe, expect, test } from 'bun:test';
import { saveOutcome } from '../../src/lib/resume';

describe('saveOutcome', () => {
  test('builds the meeting url', () => {
    expect(saveOutcome({ meeting_id: 'm1' }).url).toBe('/meeting-details?id=m1&source=recording');
  });

  test('a continued save adds continued=1 to the url', () => {
    const outcome = saveOutcome({ meeting_id: 'm1', continued: true, continue_error: null });
    expect(outcome.url).toBe('/meeting-details?id=m1&source=recording&continued=1');
    expect(outcome.continued).toBe(true);
    expect(outcome.continueError).toBeNull();
  });

  test('surfaces continue_error', () => {
    const outcome = saveOutcome({ meeting_id: 'm2', continue_error: 'Merging the audio failed: x' });
    expect(outcome.continueError).toBe('Merging the audio failed: x');
    expect(outcome.continued).toBe(false);
  });
});
