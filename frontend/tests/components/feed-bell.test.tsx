import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  navigation: { ...await import('next/navigation') },
  popover: { ...await import('../../src/components/ui/popover') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('next/navigation', () => originals.navigation);
  mock.module('../../src/components/ui/popover', () => originals.popover);
});

interface Item {
  id: string;
  kind: string;
  title: string;
  targetId: string;
  createdAt: string;
  read: boolean;
}

let items: Item[] = [];
let unread = 0;
let listener: (() => void) | null = null;

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'feed_unread_count') return unread;
  if (command === 'feed_list') return structuredClone(items);
  if (command === 'feed_mark_read' || command === 'feed_mark_all_read') return undefined;
  throw new Error(`Unexpected command: ${command} ${JSON.stringify(args ?? {})}`);
});
const push = mock((_href: string) => {});
const listen = mock(async (_event: string, handler: () => void) => {
  listener = handler;
  return () => {};
});

mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('@tauri-apps/api/event', () => ({ listen }));
mock.module('next/navigation', () => ({ ...originals.navigation, useRouter: () => ({ push }) }));
// Pass-through so the open content is always in the tree; Radix portals do not render under react-test-renderer
const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
mock.module('../../src/components/ui/popover', () => ({
  Popover: passThrough,
  PopoverTrigger: passThrough,
  PopoverContent: passThrough,
}));

const { FeedBell } = await import('../../src/components/Feed/FeedBell');

let renderer: ReactTestRenderer | null = null;

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

async function mount() {
  renderer?.unmount();
  renderer = null;
  await act(async () => {
    renderer = create(<FeedBell />);
  });
  await flush();
}

function text(node: ReactTestInstance): string {
  return node.children.map((child) => (typeof child === 'string' ? child : text(child))).join('');
}

function bell(): ReactTestInstance {
  return renderer!.root.find((n) => n.type === 'button' && n.props['aria-label'] === 'Notifications');
}

function item(title: string): ReactTestInstance {
  return renderer!.root.find((n) => n.type === 'button' && text(n) === title);
}

const brief: Item = {
  id: 'feed-1',
  kind: 'brief',
  title: 'Brief: 1-2-1 Ivan at 11:00',
  targetId: 'brief-1',
  createdAt: '2026-10-22T09:00:00.000Z',
  read: false,
};

beforeEach(() => {
  items = [];
  unread = 0;
  listener = null;
  invoke.mockClear();
  push.mockClear();
  listen.mockClear();
});

describe('FeedBell badge', () => {
  test('shows the unread count', async () => {
    unread = 3;
    items = [brief];
    await mount();
    expect(text(bell())).toBe('3');
  });

  test('shows 9+ above nine', async () => {
    unread = 12;
    items = [brief];
    await mount();
    expect(text(bell())).toBe('9+');
  });

  test('is hidden at zero', async () => {
    await mount();
    expect(text(bell())).toBe('');
  });

  test('refetches when feed-updated fires', async () => {
    await mount();
    expect(text(bell())).toBe('');
    unread = 1;
    items = [brief];
    await act(async () => {
      listener?.();
    });
    await flush();
    expect(text(bell())).toBe('1');
  });
});

describe('FeedBell list', () => {
  test('shows the empty state', async () => {
    await mount();
    expect(text(renderer!.root.findByType('p'))).toBe('No notifications');
  });

  test('clicking an item marks it read and routes to its target', async () => {
    unread = 1;
    items = [brief];
    await mount();
    await act(async () => {
      item(brief.title).props.onClick();
    });
    await flush();
    expect(invoke).toHaveBeenCalledWith('feed_mark_read', { id: 'feed-1' });
    expect(push).toHaveBeenCalledWith('/briefs?id=brief-1&kind=brief');
  });

  test('mark all as read calls the backend', async () => {
    unread = 1;
    items = [brief];
    await mount();
    const button = renderer!.root.find((n) => n.type === 'button' && text(n) === 'Mark all as read');
    await act(async () => {
      button.props.onClick();
    });
    await flush();
    expect(invoke).toHaveBeenCalledWith('feed_mark_all_read');
  });

  test('renders the title as literal text', async () => {
    const hostile = { ...brief, id: 'feed-2', title: '<img src=x onerror=alert(1)> <b>bold</b>' };
    unread = 1;
    items = [hostile];
    await mount();
    expect(item(hostile.title)).toBeTruthy();
    expect(renderer!.root.findAll((n) => n.type === 'img' || n.type === 'b')).toHaveLength(0);
  });
});
