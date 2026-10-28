import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = { core: { ...await import('@tauri-apps/api/core') } };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
});

let view: Record<string, unknown> = {};
const invoke = mock(async (command: string): Promise<unknown> => {
  if (command === 'get_meeting_banner') return view;
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

const buttons = (r: ReactTestRenderer) => r.root.findAll((n) => n.type === 'button');

beforeEach(() => {
  invoke.mockClear();
  view = {};
});

describe('banner continue', () => {
  test('detected with an offer shows Record new and Continue', async () => {
    view = { kind: 'detected', app: 'Zoom', certain: true, line: null, continue_title: 'Standup' };
    const r = await render();
    const labels = buttons(r).map(textOf);
    expect(labels).toContain('Record new');
    expect(labels).toContain('Continue "Standup"');
    expect(labels).not.toContain('Record');
    const cont = buttons(r).find((b) => textOf(b) === 'Continue "Standup"')!;
    await act(async () => { await cont.props.onClick(); });
    expect(invoke).toHaveBeenCalledWith('meeting_banner_action', { action: 'continue' });
  });

  test('detected without an offer still shows Record', async () => {
    view = { kind: 'detected', app: 'Zoom', certain: true, line: null, continue_title: null };
    const r = await render();
    const labels = buttons(r).map(textOf);
    expect(labels).toContain('Record');
    expect(labels).not.toContain('Record new');
  });

  test('continue banner shows only Continue and dismiss', async () => {
    view = { kind: 'continue', app: 'Zoom', certain: true, line: null, continue_title: 'Standup' };
    const r = await render();
    const text = JSON.stringify(r.toJSON());
    expect(text).toContain('Zoom: still the same call?');
    const labels = buttons(r).map((b) => b.props['aria-label'] ?? textOf(b));
    expect(labels).toEqual(['Continue "Standup"', 'Dismiss']);
  });
});
