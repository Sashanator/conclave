import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  sonner: { ...await import('sonner') },
  dialog: { ...await import('../../src/components/ui/dialog') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/components/ui/dialog', () => originals.dialog);
});

const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
const items = [
  { meeting_id: 'm1', title: 'Standup', created_at: '2026-08-01T09:00:00Z', bytes: 12_400_000, files: ['/rec/m1/audio.mp4'] },
  { meeting_id: 'm2', title: 'Retro', created_at: '2026-08-02T09:00:00Z', bytes: 3_000_000, files: ['/rec/m2/audio.wav', '/rec/m2/.checkpoints/audio_chunk_0.wav'] },
];
const deleteResult = { meetings: 1, bytes: 12_400_000, failed: [] as string[] };
const invoke = mock(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'old_audio_list') return items;
  if (command === 'delete_old_audio') return deleteResult;
  throw new Error(`Unexpected command: ${command}`);
});
const toast = { success: mock((_what: string, _opts?: unknown) => {}), error: mock((_what: string, _opts?: unknown) => {}), warning: mock((_what: string, _opts?: unknown) => {}) };

mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('sonner', () => ({ toast }));
mock.module('../../src/components/ui/dialog', () => ({
  Dialog: passThrough, DialogContent: passThrough, DialogDescription: passThrough, DialogFooter: passThrough,
  DialogHeader: passThrough, DialogTitle: passThrough,
}));

const { OldAudioReview } = await import('../../src/components/OldAudio/OldAudioReview');
const { AUDIO_DELETED_EVENT } = await import('../../src/lib/meeting-audio');

let renderer: ReactTestRenderer | null = null;

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

function text(node: ReactTestInstance): string {
  return node.children.map((child) => (typeof child === 'string' ? child : text(child))).join('');
}

function deleteButton(): ReactTestInstance {
  return renderer!.root.findAll((n) => n.type === 'button' && text(n).startsWith('Move'))[0];
}

function button(label: string): ReactTestInstance {
  return renderer!.root.findAll((n) => n.type === 'button' && text(n) === label)[0];
}

function checkboxes(): ReactTestInstance[] {
  return renderer!.root.findAll((n) => n.type === 'input' && n.props.type === 'checkbox');
}

async function tick(index: number) {
  await act(async () => {
    checkboxes()[index].props.onChange();
  });
}

describe('old-audio review', () => {
  beforeEach(async () => {
    invoke.mockClear();
    toast.success.mockClear();
    toast.warning.mockClear();
    renderer?.unmount();
    renderer = null;
    await act(async () => {
      renderer = create(<OldAudioReview open onOpenChange={() => {}} />);
    });
    await flush();
  });

  test('nothing is checked by default and the delete button is disabled', () => {
    expect(checkboxes().map((c) => c.props.checked)).toEqual([false, false]);
    expect(deleteButton().props.disabled).toBe(true);
  });

  test('ticking one meeting enables delete and sends only that id', async () => {
    await tick(0);
    expect(deleteButton().props.disabled).toBe(false);
    await act(async () => {
      deleteButton().props.onClick();
    });
    await flush();
    expect(invoke).toHaveBeenCalledWith('delete_old_audio', { meetingIds: ['m1'] });
  });

  test('select all then delete sends both ids', async () => {
    await act(async () => {
      button('Select all').props.onClick();
    });
    await act(async () => {
      deleteButton().props.onClick();
    });
    await flush();
    expect(invoke).toHaveBeenCalledWith('delete_old_audio', { meetingIds: ['m1', 'm2'] });
  });

  test('shows the exact file paths of each meeting', () => {
    const rendered = JSON.stringify(renderer!.toJSON());
    expect(rendered).toContain('/rec/m1/audio.mp4');
    expect(rendered).toContain('/rec/m2/audio.wav');
    expect(rendered).toContain('/rec/m2/.checkpoints/audio_chunk_0.wav');
  });

  test('a failed meeting shows a warning toast', async () => {
    invoke.mockImplementationOnce(async () => ({ meetings: 0, bytes: 0, failed: ['Standup: user declined'] }));
    await tick(0);
    await act(async () => {
      deleteButton().props.onClick();
    });
    await flush();
    expect(toast.warning).toHaveBeenCalledWith('Not moved to the Recycle Bin', { description: 'Standup: user declined' });
  });

  test('a successful delete dispatches the audio-deleted event on window', async () => {
    let dispatched = 0;
    const listener = () => { dispatched += 1; };
    window.addEventListener(AUDIO_DELETED_EVENT, listener);
    try {
      await tick(0);
      await act(async () => {
        deleteButton().props.onClick();
      });
      await flush();
      expect(dispatched).toBe(1);
    } finally {
      window.removeEventListener(AUDIO_DELETED_EVENT, listener);
    }
  });
});
