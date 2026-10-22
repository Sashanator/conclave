import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  navigation: { ...await import('next/navigation') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('next/navigation', () => originals.navigation);
});

const LIST = [
  { id: 'b1', kind: 'brief', title: '1:1 Ivan', createdAt: '2026-10-22T09:00:00Z' },
  { id: 'd1', kind: 'digest', title: 'Weekly digest: 16–21 Oct', createdAt: '2026-10-21T09:00:00Z' },
];

const BRIEF = {
  eventTitle: '1:1 Ivan',
  eventStart: '2026-10-26T11:00:00Z',
  templateName: '1:1',
  previousMeetingId: 'm-old',
  previousTitle: '<img src=x> Ivan sync',
  previousDate: '2026-10-19T10:00:00Z',
  requests: ['<b>Send the deck</b>'],
  decisions: ['Ship the plan'],
  openTasks: [{ id: 't1', meetingId: 'm-old', meetingTitle: 'x', meetingDate: null, text: 'Book <i>room</i>', owner: 'me', due: null, at: null, status: 'open', source: 'auto', createdAt: 't' }],
};

const DIGEST = {
  periodStart: '2026-10-16T09:00:00Z',
  periodEnd: '2026-10-23T09:00:00Z',
  markdown: '## Decisions\n- Ship the plan [M1]\n- Chase it [M9]\n<script>alert(1)</script>\n![pixel](https://example.com/p.png)\n[click](https://evil.example)',
  meetingLinks: [{ id: 'm-old', title: 'Ivan sync' }],
};

let searchQuery = '';
const briefGets: unknown[] = [];
const digestGets: unknown[] = [];

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'briefs_list') return structuredClone(LIST);
  if (command === 'brief_get') {
    briefGets.push(args);
    return structuredClone(BRIEF);
  }
  if (command === 'digest_get') {
    digestGets.push(args);
    return structuredClone(DIGEST);
  }
  if (command === 'tasks_set_status') return null;
  throw new Error(`Unexpected command: ${command}`);
});

mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('next/navigation', () => ({
  ...originals.navigation,
  useSearchParams: () => new URLSearchParams(searchQuery),
  useRouter: () => ({ push: () => {} }),
}));

const { default: BriefsPage } = await import('../../src/app/briefs/page');

let renderer: ReactTestRenderer | null = null;

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

function textOf(node: ReactTestInstance): string {
  return node.children.map((c) => (typeof c === 'string' ? c : textOf(c))).join('');
}

async function mount(query = '') {
  renderer?.unmount();
  renderer = null;
  searchQuery = query;
  await act(async () => {
    renderer = create(<BriefsPage />);
  });
  await flush();
}

function buttonNamed(text: string): ReactTestInstance {
  const found = renderer!.root.findAll((n) => n.type === 'button' && textOf(n).trim().startsWith(text));
  if (found.length === 0) throw new Error(`no button "${text}"`);
  return found[0];
}

async function click(node: ReactTestInstance) {
  await act(async () => {
    await node.props.onClick();
  });
  await flush();
}

beforeEach(() => {
  invoke.mockClear();
  briefGets.length = 0;
  digestGets.length = 0;
});

describe('Briefs & digests page', () => {
  test('lists the briefs and filters by kind', async () => {
    await mount();
    expect(invoke).toHaveBeenCalledWith('briefs_list');
    expect(textOf(buttonNamed('1:1 Ivan'))).toContain('1:1 Ivan');
    expect(renderer!.root.findAll((n) => n.type === 'button' && textOf(n).includes('Weekly digest'))).toHaveLength(0);

    await click(buttonNamed('Digests'));
    expect(textOf(buttonNamed('Weekly digest'))).toContain('Weekly digest: 16–21 Oct');
    expect(renderer!.root.findAll((n) => n.type === 'button' && textOf(n).includes('1:1 Ivan'))).toHaveLength(0);
  });

  test('?id= opens that brief', async () => {
    await mount('id=b1&kind=brief');
    expect(briefGets).toEqual([{ id: 'b1' }]);
    const article = renderer!.root.findAll((n) => n.type === 'article');
    expect(article).toHaveLength(1);
    expect(textOf(article[0])).toContain('Ship the plan');
    expect(textOf(article[0])).toContain('From last time');
  });

  test('brief text renders literally, never as markup', async () => {
    await mount('id=b1&kind=brief');
    expect(renderer!.root.findAll((n) => n.type === 'img' || n.type === 'b' || n.type === 'i')).toHaveLength(0);
    const all = JSON.stringify(renderer!.toJSON());
    expect(all).toContain('<img src=x> Ivan sync');
    expect(all).toContain('<b>Send the deck</b>');
    expect(all).toContain('Book <i>room</i>');
  });

  test('unknown tag left as text', async () => {
    await mount('id=d1&kind=digest');
    expect(digestGets).toEqual([{ id: 'd1' }]);
    // [M1] links to its meeting; [M9] has no meeting, so it stays plain text.
    const links = renderer!.root.findAll((n) => n.type === 'a');
    expect(links.map((l) => l.props.href)).toEqual(['/meeting-details?id=m-old']);
    expect(JSON.stringify(renderer!.toJSON())).toContain('[M9]');
  });

  test('switching tabs clears the selection', async () => {
    await mount('id=d1&kind=digest');
    expect(renderer!.root.findAll((n) => n.type === 'article')).toHaveLength(1);

    await click(buttonNamed('Briefs'));
    expect(renderer!.root.findAll((n) => n.type === 'article')).toHaveLength(0);

    await click(buttonNamed('Digests'));
    expect(renderer!.root.findAll((n) => n.type === 'article')).toHaveLength(0);
  });

  test('digest markdown is sanitised', async () => {
    await mount('id=d1&kind=digest');
    expect(renderer!.root.findAll((n) => n.type === 'script' || n.type === 'img')).toHaveLength(0);
    const all = JSON.stringify(renderer!.toJSON());
    expect(all).not.toContain('<script>');
    expect(all).not.toContain('https://example.com/p.png');
    expect(all).not.toContain('https://evil.example');
    expect(all).toContain('Ship the plan');
  });
});
