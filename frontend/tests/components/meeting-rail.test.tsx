import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';
import { readSidebarOpen } from '../../src/lib/ai-sidebar';
import { AiSidebarProvider, useAiSidebarScreen } from '../../src/components/AiSidebar/AiSidebarProvider';
const originalNav = { ...(await import('next/navigation')) };
afterAll(() => { mock.module('next/navigation', () => originalNav); });
mock.module('next/navigation', () => ({ useRouter: () => ({ push() {} }) }));
import { AskChat } from '../../src/components/Ask/AskChat';
import { AiSidebar } from '../../src/components/AiSidebar/AiSidebar';
import { MeetingRail } from '../../src/components/MeetingPage/MeetingRail';

function Screen({ id }: { id: string }) {
  useAiSidebarScreen('meeting', { label: 'This meeting', meetingIds: [id] });
  return null;
}
function press(init: Partial<KeyboardEventInit> & { key: string }) {
  const e = new KeyboardEvent('keydown', { ...init, cancelable: true });
  act(() => { window.dispatchEvent(e); });
}
const tree = (id: string) => (
  <AiSidebarProvider>
    <Screen id={id} />
    <MeetingRail tasks={<p>T</p>} taskCount={3} />
    <AiSidebar />
  </AiSidebarProvider>
);
async function mount(id = 'a') {
  let r!: ReturnType<typeof create>;
  await act(async () => { r = create(tree(id)); });
  roots.push(r);
  return r;
}
const tabs = (r: ReturnType<typeof create>) => r.root.findAllByProps({ role: 'tab' }).filter((n) => typeof n.type === 'string');
const text = (n: { children: unknown[] }): string => n.children.map((c) => (typeof c === 'string' ? c : text(c as { children: unknown[] }))).join('');

beforeEach(() => localStorage.clear());
const roots: ReturnType<typeof create>[] = [];
afterEach(() => { act(() => roots.splice(0).forEach((r) => r.unmount())); localStorage.clear(); });

describe('MeetingRail', () => {
  test('tasks tab first, labelled with the count', async () => {
    const r = await mount();
    const t = tabs(r);
    expect(t.map(text)).toEqual(['Tasks (3)', 'Ask']);
    expect(t[0].props['aria-selected']).toBe(true);
    expect(r.root.findByProps({ id: 'rail-panel-ask' }).props.hidden).toBe(true);
  });

  test('Ctrl J switches the rail to Ask; no second aside', async () => {
    const r = await mount();
    press({ key: 'j', ctrlKey: true });
    expect(tabs(r)[1].props['aria-selected']).toBe(true);
    expect(r.root.findAllByProps({ 'aria-label': 'AI sidebar' }).length).toBe(0);
  });

  test('Escape on the meeting screen keeps Ask open', async () => {
    const r = await mount();
    press({ key: 'j', ctrlKey: true });
    press({ key: 'Escape' });
    expect(tabs(r)[1].props['aria-selected']).toBe(true);
  });

  test('clicking Tasks closes Ask and persists', async () => {
    const r = await mount();
    press({ key: 'j', ctrlKey: true });
    expect(readSidebarOpen('meeting')).toBe(true);
    act(() => { tabs(r)[0].props.onClick(); });
    expect(readSidebarOpen('meeting')).toBe(false);
  });

  test('no node inside the rail is an esc layer', async () => {
    const r = await mount();
    expect(r.root.findAll((n) => typeof n.type === 'string' && n.props['data-esc-layer'] !== undefined).length).toBe(0);
  });

  test('a different meeting scope starts a fresh conversation', async () => {
    let r!: ReturnType<typeof create>;
    await act(async () => { r = create(tree('a')); });
    roots.push(r);
    const keyOf = () => (r.root.findByType(AskChat) as unknown as { _fiber: { key: string } })._fiber.key;
    const first = keyOf();
    await act(async () => { r.update(tree('b')); });
    expect(keyOf()).not.toBe(first);
  });
});
