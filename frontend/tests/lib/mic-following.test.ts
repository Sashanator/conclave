import { describe, expect, test } from 'bun:test';
import { micLine, reduceMicFollowing, initialMicFollowing } from '../../src/lib/mic-following';

describe('mic following line', () => {
  test('shows the selected mic until following starts', () => {
    expect(micLine(initialMicFollowing, 'Shure MV7')).toBe('🎙 Shure MV7');
    expect(micLine(initialMicFollowing, null)).toBe('🎙 Default microphone');
  });

  test('following names the app; a later fallback switch drops it', () => {
    let s = reduceMicFollowing(initialMicFollowing, { type: 'following', device: 'Maono DM40', app: 'Zoom' });
    expect(micLine(s, 'Shure MV7')).toBe('🎙 Maono DM40 (following Zoom)');
    s = reduceMicFollowing(s, { type: 'switched', device: 'Realtek Mic' });
    expect(micLine(s, 'Shure MV7')).toBe('🎙 Realtek Mic');
  });

  test('warning is kept until a successful follow or a new recording', () => {
    let s = reduceMicFollowing(initialMicFollowing, { type: 'warning', message: 'Couldn’t tell' });
    expect(s.warning).toBe('Couldn’t tell');
    s = reduceMicFollowing(s, { type: 'following', device: 'A', app: 'Zoom' });
    expect(s.warning).toBeNull();
    s = reduceMicFollowing(reduceMicFollowing(s, { type: 'warning', message: 'x' }), { type: 'reset' });
    expect(s).toEqual(initialMicFollowing);
  });
});
