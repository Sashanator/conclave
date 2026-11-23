import { afterAll, afterEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { PillView } from '../../src/lib/dictatePill';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
});

type Handler = (e: { payload: PillView }) => void;
const handlers = new Map<string, Handler>();
let snapshot: PillView | null = null;
const invoke = mock(async (cmd: string) => (cmd === 'dictate_pill_view' ? snapshot : null));
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));
mock.module('@tauri-apps/api/event', () => ({
  ...originals.event,
  listen: async (event: string, handler: Handler) => {
    handlers.set(event, handler);
    return () => handlers.delete(event);
  },
}));

const { default: DictatePill } = await import('../../src/app/dictate-pill/page');

let renderer: ReactTestRenderer | undefined;
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  snapshot = null;
  handlers.clear();
});

async function mount() {
  await act(async () => {
    renderer = create(<DictatePill />, {
      createNodeMock: () => ({ offsetHeight: 50, scrollHeight: 44, clientHeight: 44 }),
    });
  });
}

async function emit(view: PillView) {
  await act(async () => handlers.get('dictate-pill')!({ payload: view }));
}

/** Every host element's className, in tree order. */
function classNames(r: ReactTestRenderer): string[] {
  const found: string[] = [];
  const walk = (node: unknown): void => {
    if (!node || typeof node !== 'object') return;
    const n = node as { props?: { className?: string }; children?: unknown[] };
    if (typeof n.props?.className === 'string') found.push(n.props.className);
    (n.children ?? []).forEach(walk);
  };
  walk(r.toJSON());
  return found;
}

const view = (state: PillView['state'], elapsed_ms = 0): PillView => ({ state, elapsed_ms, level: 0.4, mode: 'cleaned-up', style: 'panel' });

describe('dictate pill page', () => {
  test('the pill carries the dark palette in both themes', async () => {
    await mount();
    await emit(view('listening'));
    const pill = classNames(renderer!).find((c) => c.includes('rounded-full') && c.includes('bg-background'));
    expect(pill).toBeDefined();
    expect(pill!.split(' ')).toContain('dark');
    expect(pill!.split(' ')).toContain('text-foreground');
  });

  test('shows the state it was sent, and Stop and Cancel only when hands-free', async () => {
    await mount();
    await emit(view('listening'));
    let text = JSON.stringify(renderer!.toJSON());
    expect(text).toContain('Listening');
    expect(text).not.toContain('aria-label');

    await emit(view('hands-free', 42000));
    text = JSON.stringify(renderer!.toJSON());
    expect(text).toContain('0:42');
    expect(text).toContain('"aria-label":"Stop"');
    expect(text).toContain('"aria-label":"Cancel"');
  });

  test('reads the current view once its listener is registered', async () => {
    snapshot = view('loading-model');
    await mount();
    expect(JSON.stringify(renderer!.toJSON())).toContain('Loading model…');
  });

  test('panel shows committed and grey volatile text, capped at three lines, and resizes', async () => {
    await mount();
    await emit({ ...view('listening'), committed: 'Hello there', volatile: 'how are' });
    const names = classNames(renderer!);
    expect(names.some((c) => c.includes('text-foreground'))).toBe(true);
    expect(names.some((c) => c.includes('text-muted-foreground'))).toBe(true);
    expect(names.some((c) => c.includes('max-h-[66px]') && c.includes('text-[15px]'))).toBe(true);
    expect(JSON.stringify(renderer!.toJSON())).not.toContain('aria-live');
    expect(invoke).toHaveBeenCalledWith('dictate_pill_resize', { width: 480, height: 66 });
  });

  test('panel shrinks back when the text is gone', async () => {
    await mount();
    await emit({ ...view('listening'), committed: 'Hi' });
    await emit({ ...view('listening'), committed: '', volatile: '' });
    expect(invoke).toHaveBeenLastCalledWith('dictate_pill_resize', { width: 360, height: 56 });
  });

  test('one-line style keeps one nowrap text line with the timer and hands-free controls', async () => {
    invoke.mockClear();
    await mount();
    await emit({ ...view('hands-free', 3000), style: 'line', committed: 'Hello' });
    const names = classNames(renderer!);
    expect(names.some((c) => c.includes('w-[544px]') && c.includes('h-10'))).toBe(true);
    expect(names.some((c) => c.includes('whitespace-nowrap') && c.includes('justify-end'))).toBe(true);
    const json = JSON.stringify(renderer!.toJSON());
    expect(json).toContain('0:03');
    expect(json).toContain('"aria-label":"Stop"');
    expect(invoke.mock.calls.some((c) => c[0] === 'dictate_pill_resize')).toBe(false);
  });

  test('Conclave theme adds the Conclave palette', async () => {
    document.documentElement.classList.add('theme-conclave');
    try {
      await mount();
      await emit(view('listening'));
      const card = classNames(renderer!).find((c) => c.includes('bg-background'));
      expect(card).toContain('theme-conclave');
    } finally {
      document.documentElement.classList.remove('theme-conclave');
    }
  });

  test('text survives a state event without text', async () => {
    await mount();
    await emit({ ...view('listening'), committed: 'Kept' });
    await emit(view('transcribing'));
    const json = JSON.stringify(renderer!.toJSON());
    expect(json).toContain('Kept');
    expect(json).toContain('Transcribing…');
  });
});
