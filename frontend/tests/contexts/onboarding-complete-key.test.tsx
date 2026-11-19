import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';

const originals = {
  core: { ...(await import('@tauri-apps/api/core')) },
  event: { ...(await import('@tauri-apps/api/event')) },
  keys: { ...(await import('../../src/lib/models-keys')) },
  sonner: { ...(await import('sonner')) },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('../../src/lib/models-keys', () => originals.keys);
  mock.module('sonner', () => originals.sonner);
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
const toastError = mock((..._a: unknown[]) => {});
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));
mock.module('@tauri-apps/api/event', () => ({ ...originals.event, listen: async () => () => {} }));
mock.module('../../src/lib/models-keys', () => ({ ...originals.keys, saveProviderKey }));
mock.module('sonner', () => ({ ...originals.sonner, toast: { ...originals.sonner.toast, error: toastError } }));

const { OnboardingProvider, useOnboarding } = await import('../../src/contexts/OnboardingContext');

let ctx!: ReturnType<typeof useOnboarding>;
function Grab() { ctx = useOnboarding(); return null; }
async function mount() {
  await act(async () => { create(<OnboardingProvider><Grab /></OnboardingProvider>); });
}

beforeEach(() => {
  calls.length = 0; failComplete = false; failSave = false;
  saveProviderKey.mockClear(); toastError.mockClear();
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

  test('a failed key save still completes and tells the user', async () => {
    failSave = true;
    await mount();
    act(() => ctx.setPendingAiKey('sk-x'));
    await act(async () => { await ctx.completeOnboarding(); });
    expect(toastError).toHaveBeenCalled();
    const [title, opts] = toastError.mock.calls[0] as [string, { description: string }];
    expect(title).toBe('Could not save the API key');
    expect(opts.description).toContain('Settings → Models & keys');
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
