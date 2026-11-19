import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  rec: { ...await import('../../src/contexts/RecordingStateContext') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.rec);
});

const invoke = mock(async (_cmd: string, _a?: unknown): Promise<unknown> => null);
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));
mock.module('@tauri-apps/api/event', () => ({ ...originals.event, listen: async () => () => {} }));
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals.rec, useRecordingState: () => ({ isRecording: false }) }));

const { SidebarProvider, useSidebar } = await import('../../src/components/Sidebar/SidebarProvider');

test('the provider keeps only collapse, server address and summary polling, and never lists meetings', async () => {
  let ctx: Record<string, unknown> = {};
  const Probe = () => { ctx = useSidebar() as unknown as Record<string, unknown>; return null; };
  await act(async () => { create(<SidebarProvider><Probe /></SidebarProvider>); });
  expect(invoke.mock.calls.some(([cmd]) => cmd === 'api_get_meetings')).toBe(false);
  expect(Object.keys(ctx).sort()).toEqual(['collapsed', 'serverAddress', 'startSummaryPolling', 'stopSummaryPolling', 'toggleCollapsed']);
});

describe('collapsed state', () => {
  const mount = async () => {
    let ctx!: { collapsed: boolean };
    const Probe = () => { ctx = useSidebar() as unknown as { collapsed: boolean }; return null; };
    await act(async () => { create(<SidebarProvider><Probe /></SidebarProvider>); });
    return () => ctx.collapsed;
  };
  const press = (init: KeyboardEventInit, target: Element = document.body) => {
    const e = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
    act(() => { target.dispatchEvent(e); });
    return e;
  };
  beforeEach(() => localStorage.clear());

  test('stored state wins after mount', async () => {
    localStorage.setItem('conclave.sidebarCollapsed', 'false');
    expect((await mount())()).toBe(false);
  });
  test('Ctrl B toggles and remembers', async () => {
    const get = await mount();
    const start = get();
    const e = press({ key: 'b', ctrlKey: true });
    expect(get()).toBe(!start);
    expect(localStorage.getItem('conclave.sidebarCollapsed')).toBe(String(!start));
    expect(e.defaultPrevented).toBe(true);
    press({ key: 'B', code: 'KeyB', ctrlKey: true });
    expect(get()).toBe(start);
  });
  test('Ctrl B while typing is left to the editor', async () => {
    const get = await mount();
    const start = get();
    const host = document.createElement('div');
    host.setAttribute('contenteditable', 'true');
    const p = document.createElement('p');
    host.appendChild(p);
    document.body.appendChild(host);
    const e = press({ key: 'b', ctrlKey: true }, p);
    host.remove();
    expect(get()).toBe(start);
    expect(e.defaultPrevented).toBe(false);
  });
  test('Ctrl Shift B does nothing', async () => {
    const get = await mount();
    const start = get();
    press({ key: 'B', ctrlKey: true, shiftKey: true });
    expect(get()).toBe(start);
  });
});
