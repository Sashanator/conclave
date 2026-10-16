import { afterAll, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create } from 'react-test-renderer';

const originalPopover = { ...await import('../../src/components/ui/popover') };
afterAll(() => mock.module('../../src/components/ui/popover', () => originalPopover));
const pass = ({ children }: { children?: ReactNode }) => <>{children}</>;
mock.module('../../src/components/ui/popover', () => ({
  Popover: ({ open, children }: { open: boolean; children?: ReactNode }) => <div data-open={String(open)}>{children}</div>,
  PopoverAnchor: pass, PopoverTrigger: pass,
  PopoverContent: pass,
}));
const { TranscriptMarkButton } = await import('../../src/components/TranscriptMarkButton');

function render(mark: 'important' | 'action' | 'followup' | null, onChange = mock(() => {})) {
  let r!: ReturnType<typeof create>;
  act(() => { r = create(<TranscriptMarkButton text="send the deck" mark={mark} onChange={onChange} />); });
  return { r, onChange };
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
});
