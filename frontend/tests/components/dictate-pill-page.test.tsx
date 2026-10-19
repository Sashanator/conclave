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
    renderer = create(<DictatePill />);
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

const view = (state: PillView['state'], elapsed_ms = 0): PillView => ({ state, elapsed_ms, level: 0.4, mode: 'cleaned-up' });

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
});
