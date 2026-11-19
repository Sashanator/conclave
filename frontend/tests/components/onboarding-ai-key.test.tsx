import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { act, create, type ReactTestInstance } from 'react-test-renderer';

const original = { ...(await import('../../src/contexts/OnboardingContext')) };
afterAll(() => {
  mock.module('../../src/contexts/OnboardingContext', () => original);
});
const goNext = mock(() => {});
const setPendingAiKey = mock((_k: string | null) => {});
mock.module('../../src/contexts/OnboardingContext', () => ({
  ...original,
  useOnboarding: () => ({ goNext, goPrevious: () => {}, goToStep: () => {}, setPendingAiKey }),
}));

const { AiKeyStep } = await import('../../src/components/onboarding/steps/AiKeyStep');

const textOf = (n: ReactTestInstance | string): string => (typeof n === 'string' ? n : n.children.map(textOf).join(' '));
const render = () => {
  let r!: ReturnType<typeof create>;
  act(() => { r = create(<AiKeyStep />); });
  return r;
};
const button = (r: ReturnType<typeof create>, label: string) => r.root.findAll((n) => n.type === 'button' && textOf(n).trim() === label)[0];

beforeEach(() => { goNext.mockClear(); setPendingAiKey.mockClear(); });

describe('AiKeyStep', () => {
  test('save is disabled until a key is typed', () => {
    expect(button(render(), 'Save and continue').props.disabled).toBe(true);
  });

  test('saving keeps the trimmed key and goes next', () => {
    const r = render();
    const input = r.root.findByType('input');
    act(() => { input.props.onChange({ target: { value: '  sk-ant-x  ' } }); });
    act(() => { button(r, 'Save and continue').props.onClick(); });
    expect(setPendingAiKey).toHaveBeenCalledWith('sk-ant-x');
    expect(goNext).toHaveBeenCalled();
  });

  test('skip goes next without a key', () => {
    const r = render();
    act(() => { button(r, 'Skip for now').props.onClick(); });
    expect(setPendingAiKey).toHaveBeenCalledWith(null);
    expect(goNext).toHaveBeenCalled();
  });

  test('the key field is a password field', () => {
    const r = render();
    const input = r.root.findByType('input');
    expect(input.props.type).toBe('password');
    const label = r.root.findAll((n) => n.type === 'label' && n.props.htmlFor === input.props.id)[0];
    expect(textOf(label)).toBe('Anthropic API key');
  });

  test('onboarding source makes no false promise', () => {
    const walk = (d: string): string[] =>
      readdirSync(d).flatMap((f) => (statSync(join(d, f)).isDirectory() ? walk(join(d, f)) : [join(d, f)]));
    for (const f of walk(join(import.meta.dir, '../../src/components/onboarding'))) {
      expect(readFileSync(f, 'utf8')).not.toMatch(/Summarization Engine|never leaves your device|no cloud required/);
    }
  });
});
