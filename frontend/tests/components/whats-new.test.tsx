import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  app: { ...await import('@tauri-apps/api/app') },
  sonner: { ...await import('sonner') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/app', () => originals.app);
  mock.module('sonner', () => originals.sonner);
});

let failing = false;
const invoke = mock(async (command: string): Promise<unknown> => {
  if (failing) throw new Error('nope');
  return command === 'open_log_file' || command === 'open_database_folder' ? null : null;
});
const toast = { error: mock((_m: string) => {}), success: mock(() => {}) };
mock.module('@tauri-apps/api/core', () => ({ invoke }));
let versionFails = false;
mock.module('@tauri-apps/api/app', () => ({ getVersion: async () => { if (versionFails) throw new Error('no version'); return '1.1.0'; } }));
mock.module('sonner', () => ({ toast }));

const { WhatsNew } = await import('../../src/components/WhatsNew/WhatsNew');

const MD = `## v2.0.0 — 2026-11-17
- **Meetings table.** One table.
- <img src=x> raw

## v1.1.0 — 2026-10-30
- Older.
`;

let r: ReactTestRenderer;
const flush = () => act(async () => { await new Promise((x) => setTimeout(x, 0)); });
const text = () => JSON.stringify(r.toJSON());

beforeEach(async () => {
  failing = false;
  invoke.mockClear();
  toast.error.mockClear();
  r?.unmount();
  await act(async () => { r = create(<WhatsNew changelog={MD} />); });
  await flush();
});

describe('WhatsNew', () => {
  test('the page is left-aligned with one h1', () => {
    const h1 = r.root.findAll((n) => n.type === 'h1');
    expect(h1.map((n) => n.children.join(''))).toEqual(["What's new"]);
    expect(r.root.findAll((n) => typeof n.props.className === 'string' && /mx-auto|max-w-2xl/.test(n.props.className))).toHaveLength(0);
  });

  test('an unreadable version says so and writes no seen key', async () => {
    globalThis.localStorage.removeItem('whats-new-seen');
    versionFails = true;
    const err = console.error;
    console.error = () => {};
    try {
      r.unmount();
      await act(async () => { r = create(<WhatsNew changelog={MD} />); });
      await flush();
    } finally {
      versionFails = false;
      console.error = err;
    }
    expect(text()).toContain('Version unknown');
    expect(globalThis.localStorage.getItem('whats-new-seen')).toBeNull();
  });
  test('only the installed release is marked and the seen key is stored', () => {
    const articles = r.root.findAllByType('article');
    expect(articles).toHaveLength(2);
    expect(articles[0].props['aria-current']).toBeUndefined();
    expect(articles[1].props['aria-current']).toBe('true');
    expect(JSON.stringify(articles[1].children.map((c) => (typeof c === 'string' ? c : '')))).not.toContain('Installed');
    expect(text().match(/Installed/g)).toHaveLength(1);
    expect(globalThis.localStorage.getItem('whats-new-seen')).toBe('1.1.0');
  });
  test('footer credit, bold and raw html as text', () => {
    expect(text()).toContain('Based on Meetily (MIT)');
    expect(r.root.findAllByType('b')[0].children).toEqual(['Meetings table.']);
    expect(r.root.findAllByType('img')).toHaveLength(0);
    expect(text()).toContain('<img src=x>');
  });
  test('Open log file invokes the command; a rejection toasts', async () => {
    const btn = () => r.root.findAllByType('button').find((b) => JSON.stringify(b.children) .includes('Open log file'))!;
    await act(async () => { btn().props.onClick(); });
    expect(invoke).toHaveBeenCalledWith('open_log_file');
    failing = true;
    await act(async () => { btn().props.onClick(); });
    await flush();
    expect(toast.error).toHaveBeenCalled();
  });
});
