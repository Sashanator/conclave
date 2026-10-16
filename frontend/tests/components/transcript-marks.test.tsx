import { afterAll, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create } from 'react-test-renderer';

const originalPopover = { ...await import('../../src/components/ui/popover') };
afterAll(() => mock.module('../../src/components/ui/popover', () => originalPopover));
const pass = ({ children }: { children?: ReactNode }) => <>{children}</>;
// Captures the close-focus handler so the test can fire it the way Radix does on close.
const closeFocus: { handler?: (e: { preventDefault(): void }) => void } = {};
mock.module('../../src/components/ui/popover', () => ({
  Popover: ({ open, children }: { open: boolean; children?: ReactNode }) => <div data-open={String(open)}>{children}</div>,
  PopoverAnchor: pass, PopoverTrigger: pass,
  PopoverContent: ({ children, onCloseAutoFocus }: { children?: ReactNode; onCloseAutoFocus?: (e: { preventDefault(): void }) => void }) => {
    closeFocus.handler = onCloseAutoFocus;
    return <>{children}</>;
  },
}));
const { TranscriptMarkButton } = await import('../../src/components/TranscriptMarkButton');

function render(mark: 'important' | 'action' | 'followup' | null, onChange = mock(() => {})) {
  let r!: ReturnType<typeof create>;
  const focus = mock(() => {});
  act(() => {
    r = create(<TranscriptMarkButton text="send the deck" mark={mark} onChange={onChange} />, {
      createNodeMock: () => ({ focus }),
    });
  });
  return { r, onChange, focus };
}
const isOpen = (r: ReturnType<typeof create>) => r.root.findByProps({ 'data-open': 'true' }) !== undefined;

describe('TranscriptMarkButton', () => {
  test('click opens, choosing a mark reports it and closes', () => {
    const { r, onChange } = render(null);
    const anchor = r.root.findByProps({ role: 'button', 'aria-haspopup': 'menu' });
    act(() => anchor.props.onClick());
    expect(isOpen(r)).toBe(true);
    act(() => r.root.findByProps({ 'aria-label': 'Mark as Action item' }).props.onClick());
    expect(onChange).toHaveBeenCalledWith('action');
    expect(r.root.findAllByProps({ 'data-open': 'true' }).length).toBe(0);
  });

  test('choosing the current mark removes it', () => {
    const { r, onChange } = render('action');
    act(() => r.root.findByProps({ 'aria-haspopup': 'menu' }).props.onClick());
    act(() => r.root.findByProps({ 'aria-label': 'Mark as Action item' }).props.onClick());
    expect(onChange).toHaveBeenCalledWith(null);
  });

  test('a click that ends a text selection does not open', () => {
    const { r } = render(null);
    const sel = window.getSelection;
    (window as any).getSelection = () => ({ toString: () => 'send' });
    act(() => r.root.findByProps({ 'aria-haspopup': 'menu' }).props.onClick());
    (window as any).getSelection = sel;
    expect(r.root.findAllByProps({ 'data-open': 'true' }).length).toBe(0);
  });

  test('keyboard: Enter and Space open, Escape closes', () => {
    const { r } = render(null);
    const anchor = () => r.root.findByProps({ 'aria-haspopup': 'menu' });
    for (const key of ['Enter', ' ']) {
      act(() => anchor().props.onKeyDown({ key, preventDefault() {} }));
      expect(r.root.findAllByProps({ 'data-open': 'true' }).length).toBe(1);
      act(() => anchor().props.onKeyDown({ key: 'Escape', preventDefault() {} }));
      expect(r.root.findAllByProps({ 'data-open': 'true' }).length).toBe(0);
    }
  });

  test('shows the mark icon before the text', () => {
    const { r } = render('important');
    expect(JSON.stringify(r.toJSON())).toContain('⭐');
  });

  test('closing the popover returns keyboard focus to the line', () => {
    const { r, focus } = render(null);
    act(() => r.root.findByProps({ 'aria-haspopup': 'menu' }).props.onClick());
    const preventDefault = mock(() => {});
    act(() => closeFocus.handler!({ preventDefault }));
    expect(preventDefault).toHaveBeenCalled();
    expect(focus).toHaveBeenCalled();
  });

  test('closing does not steal focus from an element the user moved to', () => {
    const { r, focus } = render(null);
    act(() => r.root.findByProps({ 'aria-haspopup': 'menu' }).props.onClick());
    const input = document.createElement('input');
    document.body.appendChild(input);
    input.focus();
    act(() => closeFocus.handler!({ preventDefault() {} }));
    input.remove();
    expect(focus).not.toHaveBeenCalled();
  });

  test('the line label names the current mark', () => {
    const label = (mark: 'important' | null) =>
      render(mark).r.root.findByProps({ 'aria-haspopup': 'menu' }).props['aria-label'];
    expect(label('important')).toBe('Important: send the deck. Change mark');
    expect(label(null)).toBe('Mark line: send the deck');
  });
});
