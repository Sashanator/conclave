import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  dialog: { ...await import('../../src/components/ui/dialog') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('../../src/components/ui/dialog', () => originals.dialog);
});

type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();
let onOpenChange: ((open: boolean) => void) | undefined;
let installImpl: () => Promise<unknown> = async () => undefined;
const invoke = mock(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'updates_install') return installImpl();
  throw new Error(`Unexpected command: ${command}`);
});
const pass = ({ children }: { children?: ReactNode }) => <>{children}</>;
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));
mock.module('@tauri-apps/api/event', () => ({
  ...originals.event,
  listen: async (event: string, handler: Handler) => {
    handlers.set(event, handler);
    return () => handlers.delete(event);
  },
}));
mock.module('../../src/components/ui/dialog', () => ({
  Dialog: ({ open, onOpenChange: change, children }: { open: boolean; onOpenChange: (o: boolean) => void; children?: ReactNode }) => {
    onOpenChange = change;
    return open ? <>{children}</> : null;
  },
  DialogContent: pass, DialogDescription: pass, DialogFooter: pass, DialogHeader: pass, DialogTitle: pass,
}));

const { UpdateDialog } = await import('../../src/components/Updates/UpdateDialog');

let renderer: ReactTestRenderer | null = null;
const text = (n: ReactTestInstance): string => n.children.map((c) => (typeof c === 'string' ? c : text(c))).join('');
const all = () => text(renderer!.root);
const button = (label: string) => renderer!.root.findAll((n) => n.type === 'button' && text(n) === label)[0];
async function flush() { await act(async () => { await new Promise((r) => setTimeout(r, 0)); }); }
async function emit(event: string, payload: unknown) { await act(async () => { handlers.get(event)?.({ payload }); }); }
async function mount() {
  await act(async () => { renderer = create(<UpdateDialog />); });
  await flush();
}
const available = { version: '1.2.0', notes: 'Fixed things\nAdded stuff' };

describe('update dialog', () => {
  beforeEach(async () => {
    invoke.mockClear();
    handlers.clear();
    installImpl = async () => undefined;
    if (renderer) await act(async () => { renderer!.unmount(); });
    renderer = null;
    await mount();
  });

  test('stays closed until update-available', () => {
    expect(all()).not.toContain('is available');
  });

  test('shows version and notes', async () => {
    await emit('update-available', available);
    expect(all()).toContain('Conclave v1.2.0 is available');
    expect(all()).toContain('Fixed things\nAdded stuff');
  });

  test('Later closes without installing', async () => {
    await emit('update-available', available);
    await act(async () => { button('Later').props.onClick(); });
    expect(all()).not.toContain('is available');
    expect(invoke).not.toHaveBeenCalled();
  });

  test('Update installs and shows progress', async () => {
    installImpl = () => new Promise(() => {});
    await emit('update-available', available);
    await act(async () => { button('Update').props.onClick(); });
    expect(invoke).toHaveBeenCalledWith('updates_install');
    await emit('updates-progress', { downloaded: 50, total: 100 });
    expect(all()).toContain('Downloading… 50%');
    expect(button('Later').props.disabled).toBe(true);
    expect(button('Update').props.disabled).toBe(true);
  });

  test('cannot be closed while installing', async () => {
    installImpl = () => new Promise(() => {});
    await emit('update-available', available);
    await act(async () => { button('Update').props.onClick(); });
    await act(async () => { onOpenChange!(false); });
    expect(all()).toContain('is available');
    await emit('update-available', { version: '9.9.9', notes: null });
    expect(all()).toContain('v1.2.0');
  });

  test('busy refusal shows the message and allows retry', async () => {
    installImpl = async () => { throw 'busy: a summary is being generated'; };
    await emit('update-available', available);
    await act(async () => { button('Update').props.onClick(); });
    expect(all()).toContain('Conclave is busy (a summary is being generated). Update when it finishes.');
    expect(button('Update').props.disabled).toBe(false);
    installImpl = () => new Promise(() => {});
    await act(async () => { button('Update').props.onClick(); });
    expect(all()).not.toContain('is busy');
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  test('failed download shows error and changes nothing', async () => {
    installImpl = async () => { throw 'signature verification failed'; };
    await emit('update-available', available);
    await act(async () => { button('Update').props.onClick(); });
    expect(all()).toContain('Update failed: signature verification failed. Nothing was changed.');
    expect(button('Later').props.disabled).toBe(false);
  });

  test('unlistens on unmount', async () => {
    expect(handlers.size).toBe(2);
    await act(async () => { renderer!.unmount(); });
    renderer = null;
    expect(handlers.size).toBe(0);
  });
});
