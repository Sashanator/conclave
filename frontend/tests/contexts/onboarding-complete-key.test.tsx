import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';

const originals = {
  core: { ...(await import('@tauri-apps/api/core')) },
  event: { ...(await import('@tauri-apps/api/event')) },
  keys: { ...(await import('../../src/lib/models-keys')) },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('../../src/lib/models-keys', () => originals.keys);
});

const calls: string[] = [];
let failComplete = false;
let failSave = false;
const invoke = mock(async (command: string): Promise<unknown> => {
  calls.push(command);
  if (command === 'complete_onboarding' && failComplete) throw new Error('boom');
  return undefined;
});
const saveProviderKey = mock(async (provider: string, key: string) => {
  calls.push(`save:${provider}:${key}`);
  if (failSave) throw new Error('nope');
});
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));
mock.module('@tauri-apps/api/event', () => ({ ...originals.event, listen: async () => () => {} }));
mock.module('../../src/lib/models-keys', () => ({ ...originals.keys, saveProviderKey }));

const { OnboardingProvider, useOnboarding } = await import('../../src/contexts/OnboardingContext');

let ctx!: ReturnType<typeof useOnboarding>;
function Grab() { ctx = useOnboarding(); return null; }
async function mount() {
  await act(async () => { create(<OnboardingProvider><Grab /></OnboardingProvider>); });
}

beforeEach(() => {
  calls.length = 0; failComplete = false; failSave = false;
  saveProviderKey.mockClear(); localStorage.clear();
});

describe('completeOnboarding with a pending key', () => {
  test('key is saved after complete_onboarding', async () => {
    await mount();
    act(() => ctx.setPendingAiKey('sk-x'));
    await act(async () => { await ctx.completeOnboarding(); });
    expect(calls.indexOf('complete_onboarding')).toBeGreaterThan(-1);
    expect(calls.indexOf('complete_onboarding')).toBeLessThan(calls.indexOf('save:claude:sk-x'));
  });

  test('no key, no save', async () => {
    await mount();
    await act(async () => { await ctx.completeOnboarding(); });
    expect(saveProviderKey).not.toHaveBeenCalled();
  });

  test('a failed key save still completes and leaves the hint flag, never the key', async () => {
    failSave = true;
    await mount();
    act(() => ctx.setPendingAiKey('sk-x'));
    await act(async () => { await ctx.completeOnboarding(); });
    expect(localStorage.getItem('conclave.aiKeySaveFailed')).toBe('1');
    expect(JSON.stringify({ ...localStorage })).not.toContain('sk-x');
  });

  test('completing without the key step shown leaves the hint, skipping does not', async () => {
    await mount();
    await act(async () => { await ctx.completeOnboarding(); });
    expect(localStorage.getItem('conclave.aiKeySaveFailed')).toBe('1');
    localStorage.clear();
    act(() => ctx.setPendingAiKey(null));
    await act(async () => { await ctx.completeOnboarding(); });
    expect(localStorage.getItem('conclave.aiKeySaveFailed')).toBeNull();
  });

  test('a saved key leaves no hint', async () => {
    await mount();
    act(() => ctx.setPendingAiKey('sk-x'));
    await act(async () => { await ctx.completeOnboarding(); });
    expect(localStorage.getItem('conclave.aiKeySaveFailed')).toBeNull();
  });

  test('a failed completion saves no key and rethrows', async () => {
    failComplete = true;
    await mount();
    act(() => ctx.setPendingAiKey('sk-x'));
    let err: unknown;
    await act(async () => { try { await ctx.completeOnboarding(); } catch (e) { err = e; } });
    expect(err).toBeDefined();
    expect(saveProviderKey).not.toHaveBeenCalled();
  });
});
