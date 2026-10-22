import { afterAll, beforeEach, describe, expect, mock, spyOn, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { TaskRow } from '../../src/lib/tasks';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
});

const BASE: TaskRow = {
  id: 't1',
  meetingId: 'm1',
  meetingTitle: 'Sprint sync',
  meetingDate: '2026-10-20T10:00:00Z',
  text: 'Send the deck',
  owner: 'me',
  due: null,
  at: null,
  status: 'open',
  source: 'auto',
  createdAt: '2026-10-20T10:00:00Z',
};

let rows: TaskRow[] = [];

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  const id = args?.id as string | undefined;
  const find = () => rows.find((r) => r.id === id);
  if (command === 'tasks_list') return structuredClone(rows.filter((r) => r.status === args?.status));
  if (command === 'tasks_set_status') {
    const r = find();
    if (r) r.status = args?.status as TaskRow['status'];
    return null;
  }
  if (command === 'tasks_edit') {
    const r = find();
    if (r) r.text = args?.text as string;
    return null;
  }
  if (command === 'tasks_claim') {
    const r = find();
    if (r) r.owner = 'me';
    return null;
  }
  if (command === 'tasks_add') {
    const added: TaskRow = { ...BASE, id: 'new', meetingId: null, meetingTitle: null, meetingDate: null, owner: 'me', source: 'manual', text: args?.text as string };
    rows.push(added);
    return structuredClone(added);
  }
  if (command === 'tasks_delete') {
    rows = rows.filter((r) => r.id !== id);
    return null;
  }
  throw new Error(`Unexpected command: ${command}`);
});

mock.module('@tauri-apps/api/core', () => ({ invoke }));

const { default: TasksPage } = await import('../../src/app/tasks/page');

let renderer: ReactTestRenderer | null = null;

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

function textOf(node: ReactTestInstance): string {
  return node.children.map((c) => (typeof c === 'string' ? c : textOf(c))).join('');
}

function buttonNamed(text: string): ReactTestInstance {
  const found = renderer!.root.findAll((n) => n.type === 'button' && textOf(n).trim() === text);
  if (found.length !== 1) throw new Error(`expected one button "${text}", found ${found.length}`);
  return found[0];
}

function byLabel(label: string): ReactTestInstance {
  const found = renderer!.root.findAll((n) => n.props['aria-label'] === label);
  if (found.length === 0) throw new Error(`no element labelled "${label}"`);
  return found[0];
}

async function mount() {
  renderer?.unmount();
  renderer = null;
  await act(async () => {
    renderer = create(<TasksPage />);
  });
  await flush();
}

async function tick(node: ReactTestInstance) {
  await act(async () => {
    await node.props.onChange({ target: { checked: true } });
  });
  await flush();
}

async function click(node: ReactTestInstance) {
  await act(async () => {
    await node.props.onClick();
  });
  await flush();
}

beforeEach(() => {
  rows = [structuredClone(BASE), { ...structuredClone(BASE), id: 't2', text: '<img src=x>', owner: 'unassigned' }];
  invoke.mockClear();
});

describe('Tasks page', () => {
  test('ticking a task calls tasks_set_status with done', async () => {
    await mount();
    await tick(byLabel('Mark done: Send the deck'));
    expect(invoke).toHaveBeenCalledWith('tasks_set_status', { id: 't1', status: 'done' });
  });

  test('Mine on an unassigned task calls tasks_claim', async () => {
    await mount();
    await click(buttonNamed('Mine'));
    expect(invoke).toHaveBeenCalledWith('tasks_claim', { id: 't2' });
  });

  test('editing a task and pressing Enter calls tasks_edit', async () => {
    await mount();
    await click(buttonNamed('Send the deck'));
    await act(async () => {
      byLabel('Edit task text').props.onChange({ target: { value: 'Send the final deck' } });
    });
    await act(async () => {
      await byLabel('Edit task text').props.onKeyDown({ key: 'Enter', preventDefault() {} });
    });
    await flush();
    expect(invoke).toHaveBeenCalledWith('tasks_edit', { id: 't1', text: 'Send the final deck' });
  });

  test('a task text that is HTML renders literally', async () => {
    await mount();
    expect(renderer!.root.findAll((n) => n.type === 'img')).toHaveLength(0);
    expect(renderer!.root.findAll((n) => textOf(n) === '<img src=x>').length).toBeGreaterThan(0);
  });

  test('the Done tab lists done tasks', async () => {
    rows[0].status = 'done';
    await mount();
    expect(renderer!.root.findAll((n) => textOf(n) === 'Send the deck').length).toBe(0);
    await click(buttonNamed('Done'));
    expect(invoke).toHaveBeenCalledWith('tasks_list', { status: 'done' });
    expect(renderer!.root.findAll((n) => textOf(n) === 'Send the deck').length).toBeGreaterThan(0);
  });

  test('a failed command shows an inline error and the list still loads', async () => {
    await mount();
    invoke.mockImplementationOnce(async () => {
      throw new Error('task not found');
    });
    const logged = spyOn(console, 'error').mockImplementation(() => {});
    await tick(byLabel('Mark done: Send the deck'));
    logged.mockRestore();
    expect(renderer!.root.findAll((n) => n.props.role === 'alert' && textOf(n).length > 0).length).toBeGreaterThan(0);
    expect(renderer!.root.findAll((n) => textOf(n) === 'Send the deck').length).toBeGreaterThan(0);
  });
});
