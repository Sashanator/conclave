import { afterEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { useBackShortcut } from '../../src/hooks/useBackShortcut';

function Probe({ onBack, enabled }: { onBack: () => void; enabled?: boolean }) {
  useBackShortcut(onBack, enabled);
  return null;
}
let r: ReactTestRenderer | null = null;
const mount = (onBack: () => void, enabled?: boolean) => { act(() => { r = create(<Probe onBack={onBack} enabled={enabled} />); }); };
afterEach(() => { act(() => { r?.unmount(); }); r = null; document.body.innerHTML = ''; });

const press = (target: Element, init: KeyboardEventInit) => {
  const e = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(e);
  return e;
};
const add = (html: string) => { const d = document.createElement('div'); d.innerHTML = html; document.body.appendChild(d); return d; };

describe('useBackShortcut', () => {
  test('fires once on Escape and Alt+ArrowLeft, and prevents the default', () => {
    const back = mock();
    mount(back);
    expect(press(document.body, { key: 'Escape' }).defaultPrevented).toBe(true);
    expect(back).toHaveBeenCalledTimes(1);
    press(document.body, { key: 'ArrowLeft', altKey: true });
    expect(back).toHaveBeenCalledTimes(2);
  });

  test('does not fire while typing', () => {
    const back = mock();
    mount(back);
    const d = add('<input/><textarea></textarea><select></select><div contenteditable="true"><p>x</p></div>');
    for (const sel of ['input', 'textarea', 'select', 'p']) press(d.querySelector(sel)!, { key: 'Escape' });
    press(d.querySelector('input')!, { key: 'ArrowLeft', altKey: true });
    expect(back).not.toHaveBeenCalled();
  });

  test('does not fire while a layer is open', () => {
    const back = mock();
    mount(back);
    for (const html of ['<div role="dialog"></div>', '<div role="alertdialog"></div>', '<div role="menu"></div>', '<div role="listbox"></div>', '<aside data-esc-layer></aside>']) {
      const d = add(html);
      press(document.body, { key: 'Escape' });
      d.remove();
    }
    expect(back).not.toHaveBeenCalled();
  });

  test('ignores modifiers, repeats and handled events', () => {
    const back = mock();
    mount(back);
    press(document.body, { key: 'Escape', ctrlKey: true });
    press(document.body, { key: 'ArrowLeft', altKey: true, shiftKey: true });
    press(document.body, { key: 'Escape', repeat: true });
    const stop = (e: Event) => e.preventDefault();
    window.addEventListener('keydown', stop, true);
    press(document.body, { key: 'Escape' });
    window.removeEventListener('keydown', stop, true);
    expect(back).not.toHaveBeenCalled();
  });

  test('disabled and unmounted do nothing', () => {
    const back = mock();
    mount(back, false);
    press(document.body, { key: 'Escape' });
    act(() => { r!.unmount(); });
    r = null;
    mount(back);
    act(() => { r!.unmount(); });
    r = null;
    press(document.body, { key: 'Escape' });
    expect(back).not.toHaveBeenCalled();
  });

  test('a changed onBack is used without re-registering', () => {
    const a = mock(), b = mock();
    mount(a);
    act(() => { r!.update(<Probe onBack={b} />); });
    press(document.body, { key: 'Escape' });
    expect(a).not.toHaveBeenCalled();
    expect(b).toHaveBeenCalledTimes(1);
  });
});
