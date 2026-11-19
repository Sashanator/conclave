import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const realNav = { ...await import('next/navigation') };
const back = mock(() => {});
const push = mock((_h: string) => {});
// Installed at run time, not load time: later-loading files mock this module too, and bun applies the last mock.
beforeAll(() => { mock.module('next/navigation', () => ({ ...realNav, useRouter: () => ({ back, push }) })); });
afterAll(() => { mock.module('next/navigation', () => realNav); });

const { BackCrumb } = await import('../../src/components/BackCrumb');
const { setUnsavedChanges } = await import('../../src/lib/unsaved-guard');

let r: ReactTestRenderer | null = null;
const text = (n: any): string => (n.children ?? []).map((c: any) => (typeof c === 'string' ? c : text(c))).join('');
const mount = async () => { await act(async () => { r = create(<BackCrumb fallback="/" fallbackLabel="Meetings" />); }); };
const button = () => r!.root.findByType('button');

beforeEach(() => { sessionStorage.clear(); setUnsavedChanges(false); back.mockClear(); push.mockClear(); });
afterEach(() => { act(() => { r?.unmount(); }); r = null; document.body.innerHTML = ''; });

describe('BackCrumb', () => {
  test('an origin that is the previous entry names it and goes back', async () => {
    sessionStorage.setItem('conclave.origin', JSON.stringify({ path: '/?from=test', label: 'Meetings / This week', len: history.length - 1 }));
    await mount();
    expect(text(button())).toContain('Meetings / This week');
    expect(text(button())).toContain('Esc');
    expect(button().props['aria-label']).toBeUndefined();
    expect(text(button())).toContain('Back to');
    await act(async () => { await button().props.onClick(); });
    expect(back).toHaveBeenCalledTimes(1);
  });

  test('no origin uses the fallback', async () => {
    await mount();
    expect(text(button())).toContain('Meetings');
    await act(async () => { await button().props.onClick(); });
    expect(push).toHaveBeenCalledWith('/');
  });

  test('Escape goes to the fallback', async () => {
    await mount();
    await act(async () => { document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })); });
    expect(push).toHaveBeenCalledWith('/');
  });

  test('Escape inside an input does nothing', async () => {
    await mount();
    const input = document.createElement('input');
    document.body.appendChild(input);
    await act(async () => { input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })); });
    expect(push).not.toHaveBeenCalled();
    expect(back).not.toHaveBeenCalled();
  });
});
