import { describe, expect, test } from 'bun:test';
import { hasSound, micSilent, SILENT_MIC_MS } from '../../src/lib/silent-mic';

describe('silent mic rule', () => {
  test('only digital silence counts; a quiet room and speech are sound', () => {
    expect(hasSound(0)).toBe(false);
    expect(hasSound(0.000001)).toBe(false);
    expect(hasSound(0.0003)).toBe(true); // a working mic's room noise, about -70 dBFS
    expect(hasSound(0.05)).toBe(true);
  });
  test('silent only after the full 10 seconds', () => {
    expect(micSilent(1000, 1000 + SILENT_MIC_MS - 1)).toBe(false);
    expect(micSilent(1000, 1000 + SILENT_MIC_MS)).toBe(true);
  });
});
