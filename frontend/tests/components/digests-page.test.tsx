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

const { formatLongDate } = await import('../../src/lib/dates');
const { readOrigin } = await import('../../src/lib/back-nav');
const { default: DigestsPage } = await import('../../src/app/digests/page');

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
    renderer = create(<DigestsPage />);
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

describe('Digests page', () => {
  test('lists only digests', async () => {
    await mount();
    expect(invoke).toHaveBeenCalledWith('briefs_list');
    expect(renderer!.root.findAll((n) => n.type === 'h1').map(textOf)).toEqual(['Digests']);
    expect(textOf(buttonNamed('Weekly digest'))).toContain('Weekly digest: 16–21 Oct');
    expect(renderer!.root.findAll((n) => n.type === 'button' && textOf(n).includes('1:1 Ivan'))).toHaveLength(0);
  });

  test('?id= opens that digest, links known meetings, sanitises markdown', async () => {
    await mount('id=d1');
    expect(digestGets).toEqual([{ id: 'd1' }]);
    const links = renderer!.root.findAll((n) => n.type === 'a');
    expect(links.map((l) => l.props.href)).toEqual(['/meeting-details?id=m-old']);
    expect(textOf(links[0])).toBe('Ivan sync');
    expect(links[0].props.className).toContain('rounded');
    expect(links[0].props.className).toContain('border');
    expect(renderer!.root.findAll((n) => n.type === 'script' || n.type === 'img')).toHaveLength(0);
    const all = JSON.stringify(renderer!.toJSON());
    expect(all).toContain('[M9]');
    expect(all).not.toContain('<script>');
    expect(all).not.toContain('https://example.com/p.png');
    expect(all).not.toContain('https://evil.example');
    expect(all).toContain('Ship the plan');
  });

  test('the period shows long dates', async () => {
    await mount('id=d1');
    const text = textOf(renderer!.root.findByType('article'));
    expect(text).toContain(formatLongDate(new Date('2026-10-16T09:00:00Z')));
    expect(text).not.toMatch(/\d+\/\d+\/\d{4}/);
  });

  test('a chip click remembers Digests as the origin', async () => {
    await mount('id=d1');
    renderer!.root.findAll((n) => n.type === 'a')[0].props.onClick({ defaultPrevented: true });
    expect(readOrigin()).toEqual({ path: '/digests?id=d1', label: 'Digests' });
  });
});
