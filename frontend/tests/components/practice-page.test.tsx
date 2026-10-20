import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import { MATERIAL_MAX_BYTES, type PracticeStatus, type SessionSummary, type SetupSummary } from '../../src/lib/practice';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  sonner: { ...await import('sonner') },
  select: { ...await import('../../src/components/ui/select') },
};

const originalConfirm = window.confirm;

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  window.confirm = originalConfirm;
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/components/ui/select', () => originals.select);
});

const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
const placeholder = ({ placeholder: text }: { placeholder?: string }) => <>{text}</>;

const THESIS: SetupSummary = {
  id: 'seed-thesis', name: 'Thesis defence', character: 'Prof. Weber, strict examiner',
  situation: "Master's thesis defence at TU Munich.", voice: 'M1', seeded: true, has_plan: false,
  materials: [{ id: 'm1', setup_id: 'seed-thesis', kind: 'pdf', name: 'thesis.pdf', bytes: 1000 }], session_count: 2,
};
const INTERVIEW: SetupSummary = {
  id: 'seed-interview', name: 'Job interview', character: 'Anna Keller, engineering manager',
  situation: 'Interview for a backend role.', voice: 'F2', seeded: true, has_plan: false, materials: [], session_count: 0,
};
const SESSION: SessionSummary = {
  id: 's1', setup_name: 'Thesis defence', started_at: '2026-10-19T10:00:00Z', duration_secs: 1080,
  report_status: 'ready', summary: '4 grammar · 31 fillers',
};
const READY: PracticeStatus = { has_claude_key: true, engine_is_whisper: true, voices_ready: true, blocked: null, running_session: null };

let status: PracticeStatus;
let setups: SetupSummary[];
let sessions: SessionSummary[];
// What practice_start does in the next call; tests replace it
let startImpl: () => Promise<unknown> = async () => 'session-1';
// Starts left pending by holdStarts(), settled by the test in any order
let pendingStarts: { resolve: (id: string) => void; reject: (err: Error) => void }[] = [];
function holdStarts() {
  startImpl = () => new Promise<string>((resolve, reject) => {
    pendingStarts.push({ resolve, reject });
  });
}

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'practice_status') return structuredClone(status);
  if (command === 'practice_list_setups') return structuredClone(setups);
  if (command === 'practice_list_sessions') return structuredClone(sessions);
  if (command === 'practice_save_setup') return null;
  if (command === 'practice_delete_session') return null;
  if (command === 'practice_delete_setup') return null;
  if (command === 'practice_add_material') return null;
  if (command === 'practice_add_text') return null;
  if (command === 'practice_download_voices') return null;
  if (command === 'practice_preview_voice') return null;
  if (command === 'practice_new_questions') return null;
  if (command === 'practice_cancel_start') return null;
  if (command === 'practice_start') return startImpl();
  throw new Error(`Unexpected command: ${command} ${JSON.stringify(args)}`);
});
const toast = { success: mock(() => {}), error: mock((_what: string, _opts?: unknown) => {}), info: mock(() => {}), warning: mock(() => {}) };

const listeners = new Map<string, (e: { payload: unknown }) => void>();
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('@tauri-apps/api/event', () => ({
  ...originals.event,
  listen: async (event: string, handler: (e: { payload: unknown }) => void) => {
    listeners.set(event, handler);
    return () => {
      if (listeners.get(event) === handler) listeners.delete(event);
    };
  },
}));
mock.module('sonner', () => ({ toast }));
mock.module('../../src/components/ui/select', () => ({
  Select: passThrough,
  SelectContent: passThrough, SelectItem: passThrough, SelectTrigger: passThrough, SelectValue: placeholder,
}));

const { default: PracticePage } = await import('../../src/app/practice/page');

let renderer: ReactTestRenderer | null = null;
const confirm = mock((_message?: string) => true);

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

async function mount() {
  renderer?.unmount();
  renderer = null;
  await act(async () => {
    renderer = create(<PracticePage />);
  });
  await flush();
}

function text(node: ReactTestInstance): string {
  return node.children.map((child) => (typeof child === 'string' ? child : text(child))).join('');
}

function buttons(label: string): ReactTestInstance[] {
  return renderer!.root.findAll((n) => n.type === 'button' && text(n) === label);
}

function clickButton(label: string) {
  return act(async () => {
    buttons(label)[0].props.onClick();
  });
}

function field(label: string): ReactTestInstance {
  return renderer!.root.findAll((n) => (n.type === 'input' || n.type === 'textarea') && n.props['aria-label'] === label)[0];
}

async function setField(label: string, value: string) {
  await act(async () => {
    field(label).props.onChange({ target: { value } });
  });
}

function calls(command: string) {
  return invoke.mock.calls.filter(([c]) => c === command);
}

beforeEach(async () => {
  status = { ...READY };
  setups = structuredClone([THESIS, INTERVIEW]);
  sessions = structuredClone([SESSION]);
  startImpl = async () => 'session-1';
  pendingStarts = [];
  invoke.mockClear();
  toast.error.mockClear();
  confirm.mockClear();
  confirm.mockImplementation(() => true);
  window.confirm = confirm as unknown as typeof window.confirm;
  listeners.clear();
  await mount();
});

describe('Practice page', () => {
  test('lists_setups_and_sessions_side_by_side', () => {
    const headings = renderer!.root.findAll((n) => n.type === 'h3').map(text);
    expect(headings).toContain('Your setups');
    expect(headings).toContain('Past sessions');
    const grid = renderer!.root.findAll((n) => n.type === 'div' && String(n.props.className).includes('lg:grid-cols-2'));
    expect(grid.length).toBe(1);
    const cards = renderer!.root.findAll((n) => n.type === 'section' && String(n.props.className).includes('bg-card'));
    expect(cards.map((c) => text(c.findAll((n) => n.type === 'h3')[0]))).toEqual(['Your setups', 'Past sessions']);
  });

  test('start_is_disabled_with_the_blocked_reason', async () => {
    status = { ...READY, blocked: 'Practice is unavailable while a meeting is recording, importing or re-transcribing' };
    await mount();
    const banner = renderer!.root.findAll((n) => n.type === 'p' && text(n) === status.blocked);
    expect(banner.length).toBe(1);
    expect(buttons('▶ Start').every((b) => b.props.disabled)).toBe(true);
  });

  test('a missing key is the banner reason and disables Start', async () => {
    status = { ...READY, has_claude_key: false };
    await mount();
    const banner = renderer!.root.findAll((n) => n.type === 'p' && text(n) === 'Add your Claude API key in Settings to use Practice');
    expect(banner.length).toBe(1);
    expect(buttons('▶ Start').every((b) => b.props.disabled)).toBe(true);
  });

  test('a session row shows the date, the minutes and the summary', () => {
    const lines = renderer!.root.findAll((n) => n.type === 'p').map(text);
    expect(lines.some((l) => l.includes('18 min') && l.includes('4 grammar · 31 fillers'))).toBe(true);
  });

  test('editor_validates_name_and_saves', async () => {
    await clickButton('Edit');
    await setField('Setup name', '   ');
    await clickButton('Save');
    expect(calls('practice_save_setup').length).toBe(0);
    expect(renderer!.root.findAll((n) => n.type === 'p' && text(n) === 'Give the setup a name').length).toBe(1);

    await setField('Setup name', '  Thesis defence v2  ');
    await setField('Character', ' Prof. Weber ');
    await clickButton('Save');
    expect(calls('practice_save_setup')[0][1]).toEqual({
      input: { id: 'seed-thesis', name: 'Thesis defence v2', character: 'Prof. Weber', situation: "Master's thesis defence at TU Munich.", voice: 'M1' },
    });
  });

  test('oversized_file_is_refused_before_upload', async () => {
    await clickButton('Edit');
    const big = { name: 'huge.pdf', size: MATERIAL_MAX_BYTES - 999, arrayBuffer: async () => new ArrayBuffer(0) };
    await act(async () => {
      renderer!.root.findAll((n) => n.type === 'input' && n.props.type === 'file')[0].props.onChange({ target: { files: [big] } });
    });
    expect(calls('practice_add_material').length).toBe(0);
    expect(toast.error).toHaveBeenCalled();
  });

  test('a file is uploaded with its setup, encoded name and kind headers', async () => {
    await clickButton('Edit');
    const file = { name: 'abstract notes.md', size: 4, arrayBuffer: async () => new TextEncoder().encode('abcd').buffer };
    await act(async () => {
      renderer!.root.findAll((n) => n.type === 'input' && n.props.type === 'file')[0].props.onChange({ target: { files: [file] } });
    });
    await flush();
    const [command, bytes, options] = calls('practice_add_material')[0] as unknown as [string, Uint8Array, unknown];
    expect(command).toBe('practice_add_material');
    expect(bytes).toEqual(new TextEncoder().encode('abcd'));
    expect(options).toEqual({ headers: { 'x-setup-id': 'seed-thesis', 'x-name': 'abstract%20notes.md', 'x-kind': 'text' } });
  });

  test('delete_session_asks_first', async () => {
    confirm.mockImplementation(() => false);
    await clickButton('Delete');
    expect(confirm).toHaveBeenCalled();
    expect(calls('practice_delete_session').length).toBe(0);

    confirm.mockImplementation(() => true);
    await clickButton('Delete');
    await flush();
    expect(calls('practice_delete_session')[0][1]).toEqual({ id: 's1' });
  });

  test('start_downloads_missing_voices_then_starts_the_session', async () => {
    status = { ...READY, voices_ready: false };
    await mount();
    await clickButton('▶ Start');
    await flush();
    expect(calls('practice_download_voices').length).toBe(1);
    expect(calls('practice_start')[0][1]).toEqual({ setupId: 'seed-thesis', skipPlan: false });
    expect(renderer!.root.findAll((n) => n.type === 'div' && text(n) === 'Call view').length).toBe(1);
  });

  test('a plan failure offers Retry and Start without questions', async () => {
    startImpl = async () => {
      listeners.get('practice-prep')?.({ payload: { stage: 'failed', error: 'Claude did not answer' } });
      throw new Error('Claude did not answer');
    };
    await clickButton('▶ Start');
    await flush();
    expect(buttons('Retry').length).toBe(1);
    expect(buttons('Start without questions').length).toBe(1);

    startImpl = async () => 'session-1';
    await clickButton('Start without questions');
    await flush();
    expect(calls('practice_start').at(-1)![1]).toEqual({ setupId: 'seed-thesis', skipPlan: true });
  });

  test('a plan rejection without a prep event still offers Retry and Start without questions', async () => {
    startImpl = async () => {
      throw new Error('Claude did not answer');
    };
    await clickButton('▶ Start');
    await flush();
    expect(buttons('Retry').length).toBe(1);
    expect(buttons('Start without questions').length).toBe(1);
    expect(renderer!.root.findAll((n) => n.type === 'p' && text(n).includes('Claude did not answer')).length).toBe(1);
    expect(toast.error).not.toHaveBeenCalled();
  });

  test('a rejection from an attempt cancelled before a new Start leaves the new overlay alone', async () => {
    holdStarts();
    await clickButton('▶ Start');
    await flush();
    await clickButton('Cancel');
    await clickButton('▶ Start');
    await flush();
    expect(pendingStarts.length).toBe(2);

    await act(async () => {
      pendingStarts[0].reject(new Error('Claude did not answer'));
    });
    await flush();
    expect(toast.error).not.toHaveBeenCalled();
    expect(buttons('Retry').length).toBe(0);
    expect(buttons('Cancel').length).toBe(1);
  });

  test('a start that resolves after Cancel does not open the call view', async () => {
    holdStarts();
    await clickButton('▶ Start');
    await flush();
    await clickButton('Cancel');
    await flush();

    await act(async () => {
      pendingStarts[0].resolve('session-1');
    });
    await flush();
    expect(renderer!.root.findAll((n) => n.type === 'div' && text(n) === 'Call view').length).toBe(0);
    expect(buttons('Cancel').length).toBe(0);
  });
});
