import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  select: { ...await import('../../src/components/ui/select') },
  switchUi: { ...await import('../../src/components/ui/switch') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('../../src/components/ui/select', () => originals.select);
  mock.module('../../src/components/ui/switch', () => originals.switchUi);
});

const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;

type View = {
  provider: string | null;
  model: string | null;
  my_names: string[];
  extract_enabled: boolean;
  digest_enabled: boolean;
};

type Calendar = {
  connected: boolean;
  host: string | null;
  last_ok_at: string | null;
  last_error: string | null;
  upcoming: number;
};

const disconnected: Calendar = { connected: false, host: null, last_ok_at: null, last_error: null, upcoming: 0 };
const connected: Calendar = {
  connected: true,
  host: 'calendar.example.com',
  last_ok_at: '2026-10-22T09:00:00Z',
  last_error: null,
  upcoming: 3,
};

const FEED = 'https://calendar.example.com/private/basic.ics';

let view: View;
let calendar: Calendar;
// Per test: how many events a connect finds, and whether reading the calendar status fails
let upcomingOnConnect: number;
let statusFails: boolean;

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'knowledge_get_settings') return structuredClone(view);
  if (command === 'knowledge_save_settings') {
    view = structuredClone(args?.input as View);
    return structuredClone(view);
  }
  if (command === 'calendar_status') {
    if (statusFails) throw 'calendar status unavailable';
    return structuredClone(calendar);
  }
  if (command === 'calendar_connect') {
    if (String(args?.url).includes('bad')) throw 'calendar feed did not parse';
    calendar = { ...structuredClone(connected), upcoming: upcomingOnConnect };
    return structuredClone(calendar);
  }
  if (command === 'calendar_disconnect') {
    calendar = structuredClone(disconnected);
    return structuredClone(calendar);
  }
  if (command === 'api_get_api_key') return '';
  if (command === 'api_get_model_config') return null;
  if (command === 'api_get_custom_openai_config') return null;
  throw new Error(`Unexpected command: ${command}`);
});

mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('../../src/components/ui/select', () => ({
  Select: passThrough,
  SelectContent: passThrough,
  SelectItem: passThrough,
  SelectTrigger: () => null,
  SelectValue: () => null,
}));
// A plain button stands in for the Radix switch, so a test can press it
mock.module('../../src/components/ui/switch', () => ({
  Switch: ({ checked, onCheckedChange, ...rest }: { checked?: boolean; onCheckedChange?: (on: boolean) => void; [key: string]: unknown }) => (
    <button type="button" role="switch" aria-checked={checked} onClick={() => onCheckedChange?.(!checked)} {...rest} />
  ),
}));

const { KnowledgeSettings } = await import('../../src/components/KnowledgeSettings');

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
    renderer = create(<KnowledgeSettings />);
  });
  await flush();
}

function textOf(node: ReactTestInstance): string {
  return node.children.map((c) => (typeof c === 'string' ? c : textOf(c))).join('');
}

function allText(): string {
  return renderer!.root.findAll(() => true).filter((n) => typeof n.type === 'string').map((n) => n.children.filter((c) => typeof c === 'string').join('')).join(' ');
}

function byLabel(label: string): ReactTestInstance {
  const found = renderer!.root.findAll((n) => n.props['aria-label'] === label && typeof n.type === 'string');
  if (found.length === 0) throw new Error(`no element labelled "${label}"`);
  return found[0];
}

function paragraphWith(text: string): ReactTestInstance {
  const found = renderer!.root.findAll((n) => n.type === 'p' && textOf(n).includes(text));
  if (found.length === 0) throw new Error(`no paragraph with "${text}"`);
  return found[0];
}

function byButton(text: string): ReactTestInstance {
  const found = renderer!.root.findAll((n) => n.type === 'button' && textOf(n) === text);
  if (found.length === 0) throw new Error(`no button "${text}"`);
  return found[0];
}

beforeEach(() => {
  view = {
    provider: null,
    model: null,
    my_names: ['Sasha'],
    extract_enabled: true,
    digest_enabled: true,
  };
  calendar = structuredClone(disconnected);
  upcomingOnConnect = 3;
  statusFails = false;
  invoke.mockClear();
});

describe('KnowledgeSettings', () => {
  test('toggling extraction saves extract_enabled false', async () => {
    await mount();
    await act(async () => {
      await byLabel('Extract my tasks after each summary').props.onClick();
    });
    await flush();
    expect(invoke).toHaveBeenCalledWith(
      'knowledge_save_settings',
      { input: expect.objectContaining({ extract_enabled: false, digest_enabled: true }) },
    );
  });

  test('adding the name Саша saves it', async () => {
    await mount();
    await act(async () => {
      byLabel('Add a name').props.onChange({ target: { value: 'Саша' } });
    });
    await act(async () => {
      await byLabel('Add a name').props.onKeyDown({ key: 'Enter', preventDefault() {} });
    });
    await flush();
    expect(invoke).toHaveBeenCalledWith(
      'knowledge_save_settings',
      { input: expect.objectContaining({ my_names: ['Sasha', 'Саша'] }) },
    );
  });

  test('the calendar card shows the host and last sync, never the URL', async () => {
    calendar = structuredClone(connected);
    await mount();
    const text = allText();
    expect(text).toContain('calendar.example.com');
    expect(text).toContain('Disconnect');
    expect(text).not.toContain('http');
    expect(text).not.toContain('://');
  });

  test('the calendar card says Not connected when disconnected', async () => {
    await mount();
    expect(allText()).toContain('Not connected');
  });

  test('the secret address input is a password field', async () => {
    await mount();
    expect(byLabel('Calendar secret address').props.type).toBe('password');
  });

  test('Connect is disabled while the address is empty', async () => {
    await mount();
    expect(byButton('Connect').props.disabled).toBe(true);
    await act(async () => {
      byLabel('Calendar secret address').props.onChange({ target: { value: FEED } });
    });
    expect(byButton('Connect').props.disabled).toBe(false);
  });

  test('Connect shows the number of upcoming events', async () => {
    await mount();
    await act(async () => {
      byLabel('Calendar secret address').props.onChange({ target: { value: FEED } });
    });
    await act(async () => {
      await byButton('Connect').props.onClick();
    });
    await flush();
    expect(invoke).toHaveBeenCalledWith('calendar_connect', { url: FEED });
    expect(allText()).toContain('Found 3 upcoming events');
  });

  test('Connect says "1 upcoming event" for a single event', async () => {
    upcomingOnConnect = 1;
    await mount();
    await act(async () => {
      byLabel('Calendar secret address').props.onChange({ target: { value: FEED } });
    });
    await act(async () => {
      await byButton('Connect').props.onClick();
    });
    await flush();
    expect(allText()).toContain('Found 1 upcoming event');
    expect(allText()).not.toContain('Found 1 upcoming events');
  });

  test('a failed Connect message is red, a successful one is not', async () => {
    await mount();
    await act(async () => {
      byLabel('Calendar secret address').props.onChange({ target: { value: 'https://calendar.example.com/bad.ics' } });
    });
    await act(async () => {
      await byButton('Connect').props.onClick();
    });
    await flush();
    expect(paragraphWith('calendar feed did not parse').props.className).toContain('text-red-600');
  });

  test('a successful Connect message is not red', async () => {
    await mount();
    await act(async () => {
      byLabel('Calendar secret address').props.onChange({ target: { value: FEED } });
    });
    await act(async () => {
      await byButton('Connect').props.onClick();
    });
    await flush();
    expect(paragraphWith('Found 3 upcoming events').props.className).not.toContain('text-red-600');
  });

  test('the connect form still shows when calendar_status fails', async () => {
    statusFails = true;
    await mount();
    expect(allText()).toContain('Not connected');
    expect(byLabel('Calendar secret address').props.type).toBe('password');
  });

  test('a successful Connect clears the address input', async () => {
    await mount();
    await act(async () => {
      byLabel('Calendar secret address').props.onChange({ target: { value: FEED } });
    });
    await act(async () => {
      await byButton('Connect').props.onClick();
    });
    await flush();
    // Connected now, so the input is gone; the next form shown after a disconnect must be empty
    await act(async () => {
      await byButton('Disconnect').props.onClick();
    });
    await flush();
    expect(byLabel('Calendar secret address').props.value).toBe('');
  });

  test('a failed Connect shows the error text and keeps the address', async () => {
    await mount();
    const bad = 'https://calendar.example.com/bad.ics';
    await act(async () => {
      byLabel('Calendar secret address').props.onChange({ target: { value: bad } });
    });
    await act(async () => {
      await byButton('Connect').props.onClick();
    });
    await flush();
    expect(allText()).toContain('calendar feed did not parse');
    expect(byLabel('Calendar secret address').props.value).toBe(bad);
  });

  test('Disconnect calls calendar_disconnect and shows the form again', async () => {
    calendar = structuredClone(connected);
    await mount();
    await act(async () => {
      await byButton('Disconnect').props.onClick();
    });
    await flush();
    expect(invoke).toHaveBeenCalledWith('calendar_disconnect');
    expect(byLabel('Calendar secret address').props.type).toBe('password');
  });
});
