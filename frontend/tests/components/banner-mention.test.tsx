import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = { core: { ...await import('@tauri-apps/api/core') } };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
});

const LINE = '…Sasha, what do you think?';
const invoke = mock(async (command: string): Promise<unknown> => {
  if (command === 'get_meeting_banner') return { kind: 'mention', app: '', certain: true, line: LINE };
  return undefined;
});
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));

const { default: MeetingBanner } = await import('../../src/app/banner/page');

const flush = () => act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
const textOf = (n: ReactTestInstance): string => n.children.map((c) => (typeof c === 'string' ? c : textOf(c))).join('');

async function render(): Promise<ReactTestRenderer> {
  let r!: ReactTestRenderer;
  act(() => { r = create(<MeetingBanner />); });
  await flush();
  return r;
}

beforeEach(() => invoke.mockClear());

describe('mention banner', () => {
  test('shows the title and the matching line', async () => {
    const r = await render();
    const text = JSON.stringify(r.toJSON());
    expect(text).toContain('Someone said your name');
    expect(text).toContain(LINE);
    expect(text).not.toContain('meeting detected');
  });

  test('clicking the text opens the copilot', async () => {
    const r = await render();
    const buttons = r.root.findAll((n) => n.type === 'button' && textOf(n).includes('Someone said your name'));
    expect(buttons.length).toBe(1);
    await act(async () => { await buttons[0].props.onClick(); });
    expect(invoke).toHaveBeenCalledWith('meeting_banner_action', { action: 'open' });
  });

  test('the X dismisses', async () => {
    const r = await render();
    const x = r.root.findAll((n) => n.type === 'button' && n.props['aria-label'] === 'Dismiss');
    expect(x.length).toBe(1);
    await act(async () => { await x[0].props.onClick(); });
    expect(invoke).toHaveBeenCalledWith('meeting_banner_action', { action: 'dismiss' });
  });
});
