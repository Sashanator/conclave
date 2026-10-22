import { describe, expect, test } from 'bun:test';

import { recoveredDescription } from '../../src/lib/recovery-toast';

describe('recoveredDescription', () => {
  test('a full audio recovery says the audio came back', () => {
    expect(recoveredDescription('success')).toBe('Transcripts and audio recovered');
  });

  test('a partial audio recovery says part of the audio came back', () => {
    expect(recoveredDescription('partial')).toBe(
      'Transcripts and part of the audio recovered (some audio could not be restored)',
    );
  });

  test('no audio, a failed recovery or a missing status says no audio is available', () => {
    expect(recoveredDescription('failed')).toBe('Transcripts recovered (no audio available)');
    expect(recoveredDescription('none')).toBe('Transcripts recovered (no audio available)');
    expect(recoveredDescription(undefined)).toBe('Transcripts recovered (no audio available)');
  });
});
