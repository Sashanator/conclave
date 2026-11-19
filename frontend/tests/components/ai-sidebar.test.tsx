import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';
import { readSidebarOpen, writeSidebarOpen, type ScreenKind } from '../../src/lib/ai-sidebar';
import { AiSidebarProvider } from '../../src/components/AiSidebar/AiSidebarProvider';
const originalNav = { ...(await import('next/navigation')) };
afterAll(() => { mock.module('next/navigation', () => originalNav); });
mock.module('next/navigation', () => ({ useRouter: () => ({ push() {} }) }));
const originalCopilot = { ...(await import('../../src/components/Copilot/CopilotPane')) };
afterAll(() => { mock.module('../../src/components/Copilot/CopilotPane', () => originalCopilot); });
mock.module('../../src/components/Copilot/CopilotPane', () => ({ CopilotPane: () => null }));
const { useRecordingState: _u, ...recCtx } = await import('../../src/contexts/RecordingStateContext');
const originalRec = { ...recCtx, useRecordingState: _u };
afterAll(() => { mock.module('../../src/contexts/RecordingStateContext', () => originalRec); });
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originalRec, useRecordingState: () => ({}) }));
const { AiSidebar } = await import('../../src/components/AiSidebar/AiSidebar');
import { useAiSidebarScreen } from '../../src/components/AiSidebar/AiSidebarProvider';

function Screen({ kind, scope }: { kind: ScreenKind | null; scope?: { label: string; meetingIds: string[] } }) {
  useAiSidebarScreen(kind, scope);
  return null;
}

function press(init: Partial<KeyboardEventInit> & { key: string }, prevent = false) {
  const e = new KeyboardEvent('keydown', { ...init, cancelable: true });
  if (prevent) e.preventDefault();
  act(() => {
    window.dispatchEvent(e);
  });
}

// Unmounted after each test: a leaked open sidebar would swallow Escape for later files.
const mounted: { unmount(): void }[] = [];
afterEach(() => { for (const m of mounted.splice(0)) act(() => m.unmount()); });

async function mount(kind: ScreenKind | null) {
  let r!: ReturnType<typeof create>;
  await act(async () => {
    r = create(
      <AiSidebarProvider>
        <Screen kind={kind} />
        <AiSidebar />
      </AiSidebarProvider>,
    );
  });
  mounted.push(r);
  return r;
}
const hasAside = (r: ReturnType<typeof create>) => r.root.findAllByType('aside').length === 1;

beforeEach(() => localStorage.clear());
afterEach(() => localStorage.clear());

describe('open state', () => {
  test('defaults and per-screen writes', () => {
    expect(readSidebarOpen('live')).toBe(true);
    expect(readSidebarOpen('table')).toBe(false);
    writeSidebarOpen('table', true);
    expect(readSidebarOpen('table')).toBe(true);
    expect(readSidebarOpen('meeting')).toBe(false);
  });
});

describe('AiSidebar', () => {
  test('Ctrl J opens, Escape closes, defaultPrevented Escape does not', async () => {
    const r = await mount('table');
    expect(hasAside(r)).toBe(false);
    press({ key: 'j', ctrlKey: true });
    expect(hasAside(r)).toBe(true);
    press({ key: 'Escape' }, true);
    expect(hasAside(r)).toBe(true);
    press({ key: 'Escape' });
    expect(hasAside(r)).toBe(false);
    // Esc that closes the sidebar must not also go Back.
    const e = new KeyboardEvent('keydown', { key: 'Escape', cancelable: true });
    press({ key: 'j', ctrlKey: true });
    act(() => { window.dispatchEvent(e); });
    expect(e.defaultPrevented).toBe(true);
    expect(readSidebarOpen('table')).toBe(false);
  });

  test('Ctrl J matches by physical key on a non-Latin layout', async () => {
    const r = await mount('table');
    press({ key: 'о', code: 'KeyJ', ctrlKey: true });
    expect(hasAside(r)).toBe(true);
  });

  test('no screen kind renders nothing', async () => {
    const r = await mount(null);
    press({ key: 'j', ctrlKey: true });
    expect(hasAside(r)).toBe(false);
  });
});

describe('live screen', () => {
  test('live: Escape does not close Copilot', async () => {
    const r = await mount('live');
    expect(hasAside(r)).toBe(true);
    press({ key: 'Escape' });
    expect(hasAside(r)).toBe(true);
  });
});

describe('esc layer', () => {
  test('esc layer only where Esc closes it', async () => {
    writeSidebarOpen('table', true);
    const t = await mount('table');
    expect(t.root.findByType('aside').props['data-esc-layer']).toBeDefined();
    const l = await mount('live');
    expect(l.root.findByType('aside').props['data-esc-layer']).toBeUndefined();
  });
});
