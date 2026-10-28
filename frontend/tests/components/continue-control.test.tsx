import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
});

let status: { offer: unknown; target: unknown } = { offer: null, target: null };
const invoke = mock(async (command: string): Promise<unknown> => {
  if (command === 'continue_status') return status;
  if (command === 'continue_set_target') return { meeting_id: 'm1', title: 'Standup' };
  return undefined;
});
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));

const handlers = new Map<string, (event: { payload: unknown }) => void>();
mock.module('@tauri-apps/api/event', () => ({
  ...originals.event,
  listen: async (event: string, handler: (event: { payload: unknown }) => void) => {
    handlers.set(event, handler);
    return () => handlers.delete(event);
  },
}));

const { ContinueControl } = await import('../../src/app/_components/ContinueControl');

const flush = () => act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
const textOf = (n: ReactTestInstance): string => n.children.map((c) => (typeof c === 'string' ? c : textOf(c))).join('');

async function render(): Promise<ReactTestRenderer> {
  let r!: ReactTestRenderer;
  act(() => { r = create(<ContinueControl />); });
  await flush();
  return r;
}

beforeEach(() => {
  invoke.mockClear();
  status = { offer: null, target: null };
});

describe('ContinueControl', () => {
  test('shows Continue for an offer and binds it on click', async () => {
    status = { offer: { meeting_id: 'm1', title: 'Standup' }, target: null };
    const r = await render();
    const buttons = r.root.findAll((n) => n.type === 'button' && textOf(n).includes('Continue "Standup"'));
    expect(buttons.length).toBe(1);
    await act(async () => { await buttons[0].props.onClick(); });
    expect(invoke).toHaveBeenCalledWith('continue_set_target', { meetingId: 'm1' });
    expect(textOf(r.root)).toContain('Continuing "Standup"');
  });

  test('shows Continuing when a target is already set', async () => {
    status = { offer: null, target: { meeting_id: 'm1', title: 'Standup' } };
    const r = await render();
    expect(textOf(r.root)).toContain('Continuing "Standup"');
    expect(r.root.findAll((n) => n.type === 'button').length).toBe(0);
  });

  test('renders nothing without offer or target', async () => {
    const r = await render();
    expect(r.toJSON()).toBeNull();
  });

  test('a continue-target-changed event sets the target', async () => {
    const r = await render();
    await act(async () => { handlers.get('continue-target-changed')!({ payload: { meeting_id: 'm1', title: 'Standup' } }); });
    expect(textOf(r.root)).toContain('Continuing "Standup"');
  });
});
