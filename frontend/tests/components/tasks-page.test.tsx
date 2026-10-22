import { afterAll, afterEach, beforeEach, describe, expect, mock, spyOn, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { TaskRow } from '../../src/lib/tasks';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  sonner: { ...await import('sonner') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
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
// When set, the next tasks_set_status waits for this before it applies, so a test can change tab meanwhile
let gateStatus: Promise<void> | null = null;

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  const id = args?.id as string | undefined;
  const find = () => rows.find((r) => r.id === id);
  if (command === 'tasks_list') return structuredClone(rows.filter((r) => r.status === args?.status));
  if (command === 'tasks_set_status') {
    if (gateStatus) {
      const wait = gateStatus;
      gateStatus = null;
      await wait;
    }
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

// sonner's toast is a function with helpers; the mock keeps the same shape
const toast = Object.assign(mock((_what: string, _opts?: unknown) => {}), {
  error: mock((_what: string, _opts?: unknown) => {}),
  dismiss: mock((_id?: unknown) => {}),
});

mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('sonner', () => ({ toast }));

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
  gateStatus = null;
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

  test('Enter on a blank edit ends the edit without calling tasks_edit', async () => {
    await mount();
    await click(buttonNamed('Send the deck'));
    await act(async () => {
      byLabel('Edit task text').props.onChange({ target: { value: '   ' } });
    });
    await act(async () => {
      await byLabel('Edit task text').props.onKeyDown({ key: 'Enter', preventDefault() {} });
    });
    await flush();
    expect(invoke.mock.calls.some(([command]) => command === 'tasks_edit')).toBe(false);
    expect(renderer!.root.findAll((n) => n.props['aria-label'] === 'Edit task text').length).toBe(0);
    expect(buttonNamed('Send the deck')).toBeDefined();
  });

  test('a reload from the old tab cannot replace the list of the tab now open', async () => {
    await mount();
    let release!: () => void;
    gateStatus = new Promise<void>((resolve) => {
      release = resolve;
    });
    let inFlight!: Promise<unknown>;
    await act(async () => {
      inFlight = byLabel('Mark done: Send the deck').props.onChange({ target: { checked: true } });
    });
    // The user moves to Done while the tick is still saving
    await click(buttonNamed('Done'));
    await act(async () => {
      release();
      await inFlight;
    });
    await flush();
    // The open-tab reload finished last, but the Done tab is showing: it must not list the open task
    expect(renderer!.root.findAll((n) => textOf(n) === '<img src=x>').length).toBe(0);
  });

  describe('delete with undo', () => {
    // The undo toast of one task, as the component handed it to sonner
    function undoToast(id: string) {
      const call = toast.mock.calls.filter(([, opts]) => (opts as { id?: string } | undefined)?.id === `task-delete-${id}`).at(-1);
      if (!call) throw new Error(`no undo toast for ${id}`);
      return call[1] as { onAutoClose: () => void; onDismiss: () => void; action: { onClick: () => void } };
    }

    // The toast closes by its timer (onAutoClose) or by the user (onDismiss)
    async function closeToast(id: string, how: 'onAutoClose' | 'onDismiss') {
      await act(async () => {
        undoToast(id)[how]();
      });
      await flush();
    }

    async function pressUndo(id: string) {
      await act(async () => {
        undoToast(id).action.onClick();
      });
      await flush();
    }

    const deletes = () => invoke.mock.calls.filter(([command]) => command === 'tasks_delete').map(([, args]) => (args as { id: string }).id);

    beforeEach(() => {
      toast.mockClear();
      toast.error.mockClear();
      toast.dismiss.mockClear();
    });

    // Unmount here, before the next test resets the rows: a pending delete runs on unmount
    afterEach(async () => {
      if (renderer) {
        await act(async () => {
          renderer!.unmount();
        });
        renderer = null;
      }
    });

    function deleteButton(index: number): ReactTestInstance {
      return renderer!.root.findAll((n) => n.props['aria-label'] === 'Delete task')[index];
    }

    function taskButton(text: string): ReactTestInstance[] {
      return renderer!.root.findAll((n) => n.type === 'button' && textOf(n) === text);
    }

    test('a click hides the task at once and shows an undo toast, with no delete yet', async () => {
      await mount();
      await click(deleteButton(0));
      expect(taskButton('Send the deck')).toHaveLength(0);
      expect(toast).toHaveBeenCalledWith('Task dismissed', expect.objectContaining({ action: expect.objectContaining({ label: 'Undo' }) }));
      expect(deletes()).toEqual([]);
    });

    test('Undo shows the task again, and the toast closing afterwards does not delete it', async () => {
      await mount();
      await click(deleteButton(0));
      await pressUndo('t1');
      expect(taskButton('Send the deck')).toHaveLength(1);
      await closeToast('t1', 'onAutoClose');
      await closeToast('t1', 'onDismiss');
      expect(deletes()).toEqual([]);
      expect(taskButton('Send the deck')).toHaveLength(1);
    });

    test('when the toast times out without Undo, tasks_delete is called once', async () => {
      await mount();
      await click(deleteButton(0));
      await closeToast('t1', 'onAutoClose');
      expect(deletes()).toEqual(['t1']);
      expect(taskButton('Send the deck')).toHaveLength(0);
    });

    test('closing the toast by hand (close button or swipe) also deletes', async () => {
      await mount();
      await click(deleteButton(0));
      await closeToast('t1', 'onDismiss');
      expect(deletes()).toEqual(['t1']);
    });

    test('Undo after the delete has started does nothing', async () => {
      await mount();
      await click(deleteButton(0));
      await act(async () => {
        undoToast('t1').onAutoClose();
        undoToast('t1').action.onClick();
      });
      await flush();
      expect(deletes()).toEqual(['t1']);
      expect(taskButton('Send the deck')).toHaveLength(0);
    });

    test('a delete never commits twice, whatever order the toast fires in', async () => {
      await mount();
      await click(deleteButton(0));
      await closeToast('t1', 'onAutoClose');
      await closeToast('t1', 'onDismiss');
      await closeToast('t1', 'onAutoClose');
      expect(deletes()).toEqual(['t1']);
    });

    test('the dismiss that follows an Undo does not delete', async () => {
      await mount();
      await click(deleteButton(0));
      await pressUndo('t1');
      await closeToast('t1', 'onDismiss');
      expect(deletes()).toEqual([]);
    });

    test('a reload that fails after a successful delete keeps the task hidden', async () => {
      await mount();
      await click(deleteButton(0));
      const original = invoke.getMockImplementation()!;
      invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
        if (command === 'tasks_list') throw new Error('offline');
        return original(command, args);
      });
      try {
        await closeToast('t1', 'onAutoClose');
      } finally {
        invoke.mockImplementation(original);
      }
      expect(deletes()).toEqual(['t1']);
      expect(taskButton('Send the deck')).toHaveLength(0);
      expect(renderer!.root.findAll((n) => n.props.role === 'alert' && textOf(n) === 'Could not load tasks.')).toHaveLength(1);
    });

    test('a manual task says "deleted", not "dismissed"', async () => {
      rows[0] = { ...rows[0], source: 'manual' };
      await mount();
      await click(deleteButton(0));
      expect(toast).toHaveBeenCalledWith('Task deleted', expect.anything());
    });

    test('a failed tasks_delete shows the task again and an error toast', async () => {
      await mount();
      await click(deleteButton(0));
      invoke.mockImplementationOnce(async () => {
        throw new Error('db locked');
      });
      const logged = spyOn(console, 'error').mockImplementation(() => {});
      await closeToast('t1', 'onAutoClose');
      logged.mockRestore();
      expect(toast.error).toHaveBeenCalledTimes(1);
      expect(taskButton('Send the deck')).toHaveLength(1);
    });

    test('several quick deletes each commit on their own toast', async () => {
      await mount();
      await click(deleteButton(0));
      await click(deleteButton(0));
      expect(taskButton('Send the deck')).toHaveLength(0);
      expect(taskButton('<img src=x>')).toHaveLength(0);
      await closeToast('t2', 'onAutoClose');
      await closeToast('t1', 'onDismiss');
      expect(deletes().sort()).toEqual(['t1', 't2']);
    });

    test('unmounting commits a pending delete once, and a later toast close does not repeat it', async () => {
      await mount();
      await click(deleteButton(0));
      const toastOpts = undoToast('t1');
      await act(async () => {
        renderer!.unmount();
      });
      renderer = null;
      await flush();
      toastOpts.onAutoClose();
      toastOpts.onDismiss();
      await flush();
      expect(deletes()).toEqual(['t1']);
    });
  });
});
