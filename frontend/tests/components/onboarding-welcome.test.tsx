import { afterAll, describe, expect, mock, test } from 'bun:test';
import { create, type ReactTestInstance } from 'react-test-renderer';

const original = { ...(await import('../../src/contexts/OnboardingContext')) };
afterAll(() => {
  mock.module('../../src/contexts/OnboardingContext', () => original);
});
mock.module('../../src/contexts/OnboardingContext', () => ({
  ...original,
  useOnboarding: () => ({ goNext: () => {}, goPrevious: () => {}, goToStep: () => {} }),
}));

const { WelcomeStep } = await import('../../src/components/onboarding/steps/WelcomeStep');

const textOf = (n: ReactTestInstance | string): string => (typeof n === 'string' ? n : n.children.map(textOf).join(' '));
const text = () => textOf(create(<WelcomeStep />).root);

describe('WelcomeStep', () => {
  test("says transcription is local and summaries use the user's key", () => {
    expect(text()).toContain('Transcription runs on this PC');
    expect(text()).toContain('Summaries use a cloud AI with your own API key');
  });

  test('makes no offline or no-cloud promise', () => {
    expect(text()).not.toMatch(/never leaves|no cloud|All on your device|offline/i);
  });
});
