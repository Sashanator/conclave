import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';
import { readSidebarOpen, writeSidebarOpen, type ScreenKind } from '../../src/lib/ai-sidebar';
import { AiSidebarProvider } from '../../src/components/AiSidebar/AiSidebarProvider';
const originalNav = { ...(await import('next/navigation')) };
afterAll(() => { mock.module('next/navigation', () => originalNav); });
mock.module('next/navigation', () => ({ useRouter: () => ({ push() {} }) }));
import { AskChat } from '../../src/components/Ask/AskChat';
import { AiSidebar } from '../../src/components/AiSidebar/AiSidebar';
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

describe('Ask chat identity', () => {
  test('a different meeting scope starts a fresh conversation', async () => {
    writeSidebarOpen('meeting', true);
    const tree = (id: string) => (
      <AiSidebarProvider>
        <Screen kind="meeting" scope={{ label: 'This meeting', meetingIds: [id] }} />
        <AiSidebar />
      </AiSidebarProvider>
    );
    let r!: ReturnType<typeof create>;
    await act(async () => { r = create(tree('a')); });
    const keyOf = () => (r.root.findByType(AskChat) as unknown as { _fiber: { key: string } })._fiber.key;
    const first = keyOf();
    await act(async () => { r.update(tree('b')); });
    expect(keyOf()).not.toBe(first);
  });
});
