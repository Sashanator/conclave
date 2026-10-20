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
const RUNNING_DETAIL = {
  summary: { ...SESSION, id: 'run-1' }, character: 'Prof. Weber, strict examiner', turns: [], report: null, report_error: null, audio_path: null,
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
  if (command === 'practice_get_session') return structuredClone(RUNNING_DETAIL);
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

function tapButtons(): ReactTestInstance[] {
  return renderer!.root.findAll((n) => n.type === 'button' && text(n).startsWith('● Tap to talk'));
}

// Starts a session from the list view; the call view is then on screen
async function startCall() {
  await clickButton('▶ Start');
  await flush();
}

async function emit(event: string, payload: unknown) {
  await act(async () => {
    listeners.get(event)?.({ payload });
  });
}

function state(phase: string, extra: Record<string, unknown> = {}) {
  return { session_id: 'session-1', phase, character: 'Prof. Weber, strict examiner', elapsed_secs: 0, caption: null, notice: null, ...extra };
}

function hasText(value: string): boolean {
  return renderer!.root.findAll((n) => n.type === 'p' || n.type === 'div' || n.type === 'span').some((n) => text(n) === value);
}

function key(init: KeyboardEventInit, target: EventTarget = window) {
  const event = new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
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
    expect(tapButtons().length).toBe(1);
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

  test('space_taps_once_per_press', async () => {
    await startCall();
    await emit('practice-state', state('UserTurn'));
    invoke.mockClear();

    const press = key({});
    expect(calls('practice_tap').length).toBe(1);
    expect(press.defaultPrevented).toBe(true);

    key({ repeat: true });
    expect(calls('practice_tap').length).toBe(1);

    const textarea = document.createElement('textarea');
    document.body.appendChild(textarea);
    key({}, textarea);
    textarea.remove();
    expect(calls('practice_tap').length).toBe(1);
  });

  test('tap_is_enabled_only_where_a_tap_means_something', async () => {
    await startCall();
    invoke.mockClear();
    expect(tapButtons()[0].props.disabled).toBe(true);
    key({});
    expect(calls('practice_tap').length).toBe(0);

    for (const phase of ['CharacterSpeaking', 'UserTurn', 'Listening', 'Thinking']) {
      await emit('practice-state', state(phase));
      expect(tapButtons()[0].props.disabled).toBe(false);
      key({});
    }
    expect(calls('practice_tap').length).toBe(4);

    await emit('practice-state', state('Transcribing'));
    expect(tapButtons()[0].props.disabled).toBe(true);
    key({});
    expect(calls('practice_tap').length).toBe(4);
  });

  test('space_with_ctrl_alt_or_meta_is_not_a_tap', async () => {
    await startCall();
    await emit('practice-state', state('UserTurn'));
    invoke.mockClear();
    for (const modifier of [{ ctrlKey: true }, { altKey: true }, { metaKey: true }]) {
      expect(key(modifier).defaultPrevented).toBe(false);
    }
    expect(calls('practice_tap').length).toBe(0);
  });

  test('avatar_shows_the_initial_of_the_last_word_of_the_name', async () => {
    await startCall();
    expect(renderer!.root.findAll((n) => n.type === 'div' && text(n) === 'W').length).toBe(1);
  });

  test('a session still running when the page opens resumes in its call view', async () => {
    status = { ...READY, running_session: 'run-1' };
    await mount();
    expect(calls('practice_get_session')[0][1]).toEqual({ id: 'run-1' });
    expect(tapButtons().length).toBe(1);
    expect(hasText('Thesis defence · 00:00')).toBe(true);
  });

  test('phase_and_notice_copy_render', async () => {
    await startCall();
    await emit('practice-state', state('CharacterSpeaking', { caption: 'Hmm. And how did you rule out overfitting?' }));
    expect(hasText('Prof. Weber is speaking…')).toBe(true);
    expect(hasText('Hmm. And how did you rule out overfitting?')).toBe(true);

    await emit('practice-state', state('UserTurn', { notice: 'DidNotCatch' }));
    expect(hasText('Your turn — tap to talk')).toBe(true);
    expect(hasText("I didn't catch that — tap and try again")).toBe(true);

    await emit('practice-state', state('Listening'));
    expect(hasText("I didn't catch that — tap and try again")).toBe(false);
    expect(hasText('Listening… tap when done')).toBe(true);
  });

  test('ten_minutes_left_notice_shows', async () => {
    await startCall();
    await emit('practice-state', state('UserTurn', { elapsed_secs: 3000, notice: 'TenMinutesLeft' }));
    expect(hasText('10 minutes left')).toBe(true);

    await emit('practice-state', state('UserTurn', { elapsed_secs: 3001 }));
    expect(hasText('10 minutes left')).toBe(true);

    await emit('practice-state', state('Thinking', { elapsed_secs: 3002 }));
    expect(hasText('10 minutes left')).toBe(false);
  });

  test('ended_switches_to_report', async () => {
    await startCall();
    await emit('practice-state', state('Ended'));
    expect(tapButtons().length).toBe(0);
    expect(renderer!.root.findAll((n) => n.type === 'div' && text(n) === 'Report view').length).toBe(1);
  });

  test('transcript_lists_this_sessions_turns_when_expanded', async () => {
    await startCall();
    await emit('practice-turn', { session_id: 'session-1', idx: 0, speaker: 'character', text: 'Good morning.' });
    await emit('practice-turn', { session_id: 'other', idx: 1, speaker: 'user', text: 'Not mine.' });
    expect(hasText('Good morning.')).toBe(false);

    await clickButton('Show full transcript ▾');
    expect(hasText('Good morning.')).toBe(true);
    expect(hasText('Not mine.')).toBe(false);
  });

  test('end_session_calls_practice_end', async () => {
    await startCall();
    await clickButton('End session → report');
    expect(calls('practice_end').length).toBe(1);
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
    expect(tapButtons().length).toBe(0);
    expect(buttons('Cancel').length).toBe(0);
  });
});
