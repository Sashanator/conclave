import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';

const originals = { sonner: { ...(await import('sonner')) } };
afterAll(() => { mock.module('sonner', () => originals.sonner); });
const toastError = mock((..._a: unknown[]) => {});
mock.module('sonner', () => ({ ...originals.sonner, toast: { ...originals.sonner.toast, error: toastError } }));

const { AiKeyHint } = await import('../../src/components/onboarding/AiKeyHint');
const mountHint = () => act(() => { create(<AiKeyHint />); });

beforeEach(() => { toastError.mockClear(); localStorage.clear(); });

describe('AiKeyHint', () => {
  test('shows the Settings hint after the reload and clears the flag', () => {
    localStorage.setItem('conclave.aiKeySaveFailed', '1');
    mountHint();
    const [title, opts] = toastError.mock.calls[0] as [string, { description: string }];
    expect(title).toBe('Could not save your Claude key');
    expect(opts.description).toContain('Settings → Models & keys');
    expect(localStorage.getItem('conclave.aiKeySaveFailed')).toBeNull();
    mountHint();
    expect(toastError).toHaveBeenCalledTimes(1);
  });

  test('shows nothing without the flag', () => {
    mountHint();
    expect(toastError).not.toHaveBeenCalled();
  });
});
