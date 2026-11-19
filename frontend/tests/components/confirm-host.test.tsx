import { afterAll, afterEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const originals = { dialog: { ...await import('../../src/components/ui/dialog') } };
afterAll(() => mock.module('../../src/components/ui/dialog', () => originals.dialog));

let onOpenChange: (o: boolean) => void = () => {};
const pass = ({ children }: { children?: ReactNode }) => <>{children}</>;
mock.module('../../src/components/ui/dialog', () => ({
  Dialog: ({ open, onOpenChange: change, children }: { open: boolean; onOpenChange: (o: boolean) => void; children?: ReactNode }) => {
    onOpenChange = change;
    return open ? <>{children}</> : null;
  },
  DialogContent: pass, DialogDescription: pass, DialogFooter: pass, DialogHeader: pass, DialogTitle: pass,
}));

const { ConfirmHost } = await import('../../src/components/ConfirmHost');
const { confirmAction } = await import('../../src/lib/confirm');
const { DESTRUCTIVE_OUTLINE } = await import('../../src/lib/utils');

let r: ReactTestRenderer | null = null;
const realConfirm = window.confirm;
afterEach(() => { act(() => { r?.unmount(); }); r = null; window.confirm = realConfirm; });

const mount = async () => { await act(async () => { r = create(<ConfirmHost />); }); };
const text = (n: any): string => (n.children ?? []).map((c: any) => (typeof c === 'string' ? c : text(c))).join('');
const confirmBtn = (label: string) => r!.root.findAll((n) => n.props.onClick && text(n) === label && n.props.variant !== undefined)[0];
const ask = (extra: object = {}) => confirmAction({ title: 'Delete the mode "A"?', body: 'It cannot be undone.', confirmLabel: 'Delete', destructive: true, ...extra });

describe('ConfirmHost', () => {
  test('confirm resolves true', async () => {
    await mount();
    let p!: Promise<boolean>;
    await act(async () => { p = ask(); });
    const t = text(r!.root);
    for (const s of ['Delete the mode "A"?', 'It cannot be undone.', 'Cancel', 'Delete']) expect(t).toContain(s);
    await act(async () => { confirmBtn('Delete').props.onClick(); });
    expect(await p).toBe(true);
    expect(r!.toJSON()).toBeNull();
  });

  test('Cancel and onOpenChange(false) resolve false', async () => {
    await mount();
    let p!: Promise<boolean>;
    await act(async () => { p = ask(); });
    await act(async () => { confirmBtn('Cancel').props.onClick(); });
    expect(await p).toBe(false);
    await act(async () => { p = ask(); });
    await act(async () => { onOpenChange(false); });
    expect(await p).toBe(false);
    expect(r!.toJSON()).toBeNull();
  });

  test('destructive uses the outline destructive button', async () => {
    await mount();
    await act(async () => { void ask(); });
    expect(confirmBtn('Delete').props.variant).toBe('outline');
    expect(confirmBtn('Delete').props.className).toBe(DESTRUCTIVE_OUTLINE);
    await act(async () => { void ask({ destructive: false, confirmLabel: 'Go' }); });
    expect(['default', undefined]).toContain(confirmBtn('Go').props.variant);
    expect(confirmBtn('Go').props.className).toBeUndefined();
  });

  test('a second request settles the first as false', async () => {
    await mount();
    let a!: Promise<boolean>, b!: Promise<boolean>;
    await act(async () => { a = ask(); });
    await act(async () => { b = ask({ title: 'Second?' }); });
    expect(await a).toBe(false);
    expect(text(r!.root)).toContain('Second?');
    await act(async () => { confirmBtn('Delete').props.onClick(); });
    expect(await b).toBe(true);
  });

  test('no host mounted falls back to window.confirm', async () => {
    const c = mock((_m?: string) => true);
    window.confirm = c as any;
    expect(await ask()).toBe(true);
    expect(c.mock.calls[0][0]).toContain('Delete the mode "A"?');
  });

  test('unmounting with a pending request resolves it false', async () => {
    await mount();
    let p!: Promise<boolean>;
    await act(async () => { p = ask(); });
    act(() => { r!.unmount(); });
    r = null;
    expect(await p).toBe(false);
  });
});
