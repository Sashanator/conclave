import { describe, expect, test } from 'bun:test';
import { requestResume, saveOutcome, takeAutoStartContinueId } from '../../src/lib/resume';

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

describe('requestResume and takeAutoStartContinueId', () => {
  test('requestResume sets both keys and opens home', () => {
    sessionStorage.clear();
    const push = (url: string) => pushed.push(url);
    const pushed: string[] = [];
    requestResume('m1', push);
    expect(sessionStorage.getItem('autoStartContinueId')).toBe('m1');
    expect(sessionStorage.getItem('autoStartRecording')).toBe('true');
    expect(pushed).toEqual(['/']);
  });

  test('takeAutoStartContinueId reads once', () => {
    sessionStorage.setItem('autoStartContinueId', 'm2');
    expect(takeAutoStartContinueId(sessionStorage)).toBe('m2');
    expect(takeAutoStartContinueId(sessionStorage)).toBeNull();
  });
});
