import { describe, expect, test } from 'bun:test';
import {
  ONBOARDING_WHISPER_MODEL,
  ONBOARDING_WHISPER_SIZE_MB,
  isOnboardingModelEvent,
  progressToDownloadedMb,
} from '../../src/lib/onboarding-transcription-model';

describe('onboarding transcription model', () => {
  test('is the Whisper default', () => {
    expect(ONBOARDING_WHISPER_MODEL).toBe('large-v3-turbo');
    expect(ONBOARDING_WHISPER_SIZE_MB).toBe(1549);
  });

  test('ignores events for other models', () => {
    expect(isOnboardingModelEvent({ modelName: 'large-v3-turbo' })).toBe(true);
    expect(isOnboardingModelEvent({ modelName: 'base' })).toBe(false);
    expect(isOnboardingModelEvent(null)).toBe(false);
  });

  test('maps percent to downloaded MB, clamped', () => {
    expect(progressToDownloadedMb(0)).toBe(0);
    expect(progressToDownloadedMb(50)).toBeCloseTo(774.5);
    expect(progressToDownloadedMb(140)).toBe(1549);
    expect(progressToDownloadedMb(-5)).toBe(0);
  });
});
