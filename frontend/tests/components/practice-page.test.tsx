import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import {
  MATERIAL_MAX_BYTES, type PracticeStatus, type Report, type SessionDetail, type SessionSummary, type SetupSummary, type TurnRow, VOICES,
} from '../../src/lib/practice';

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
  report_status: 'ready', summary: '4 grammar · 31 fillers', end_reason: 'user',
};
const RUNNING_DETAIL = {
  summary: { ...SESSION, id: 'run-1', end_reason: null }, character: 'Prof. Weber, strict examiner', turns: [], report: null, report_error: null, audio_path: null, recurring: null,
};
const READY: PracticeStatus = { has_claude_key: true, engine_is_whisper: true, voices_ready: true, blocked: null, running_session: null };
const HINT = { turn: 3, word: 'will', start_secs: 1.5, end_secs: 1.9, score: -7.2, phone: 'IH' };
const REPORT: Report = {
  version: 1,
  fluency: { fillers: { um: 14, 'so basically': 9, like: 8 }, filler_total: 31, words: 1200, speech_secs: 640, wpm: 112.4, long_pauses: 6 },
  items: [
    { kind: 'Grammar', turn: 3, quote: 'If I will have more data…', better: 'If I had more data…', note: 'Past simple after "if".', echoed: true, pattern: null },
    { kind: 'Grammar', turn: 5, quote: 'The data depend', better: 'The data depends', note: 'Third person singular.', echoed: false, pattern: null },
    { kind: 'Unnatural', turn: 7, quote: 'I made a research about it', better: 'I did some research on it', note: 'Native speakers "do" research.', echoed: false, pattern: null },
    { kind: 'Clarity', turn: 9, quote: 'So basically the idea was that, well, the baseline', better: 'The baseline missed long-range links.', note: 'Shorter.', echoed: false, pattern: null },
  ],
  dropped_items: 0,
  pronunciation: [],
  pronunciation_note: null,
};
const TURNS: TurnRow[] = [
  { session_id: 's1', idx: 0, speaker: 'character', live_text: 'Tell me about your thesis.', verbatim_text: null, at_secs: 0, wav_offset: null, wav_len: null },
  { session_id: 's1', idx: 3, speaker: 'user', live_text: 'If I will have more data', verbatim_text: 'Um, if I will have more data', at_secs: 372, wav_offset: 160_000, wav_len: 32_000 },
];
const DETAIL: SessionDetail = {
  summary: { ...SESSION, report_status: 'ready' }, character: 'Prof. Weber, strict examiner', turns: TURNS,
  report: REPORT, report_error: null, audio_path: 'C:/Conclave/practice/s1/you.wav', recurring: null,
};

let status: PracticeStatus;
let setups: SetupSummary[];
let sessions: SessionSummary[];
let detail: SessionDetail;
// The running session the resume tests open (run-1)
let running: SessionDetail;
// What practice_start does in the next call; tests replace it
let startImpl: () => Promise<unknown> = async () => 'session-1';
// Starts left pending by holdStarts(), settled by the test in any order
let pendingStarts: { resolve: (id: string) => void; reject: (err: Error) => void }[] = [];
// What practice_retry_report does; a test replaces it to hold the call open
let retryImpl: () => Promise<unknown> = async () => null;
// What practice_download_voices does; a test replaces it to hold the download open
let downloadImpl: () => Promise<unknown> = async () => null;
// What practice_speak_word does; a test replaces it to hold the call open
let speakImpl: () => Promise<unknown> = async () => null;
function holdStarts() {
  startImpl = () => new Promise<string>((resolve, reject) => {
    pendingStarts.push({ resolve, reject });
  });
}

const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  trace.push(`invoke:${command}`);
  if (command === 'practice_status') return structuredClone(status);
  if (command === 'practice_list_setups') return structuredClone(setups);
  if (command === 'practice_list_sessions') return structuredClone(sessions);
  if (command === 'practice_save_setup') return null;
  if (command === 'practice_delete_session') return null;
  if (command === 'practice_delete_setup') return null;
  if (command === 'practice_add_material') return null;
  if (command === 'practice_add_text') return null;
  if (command === 'practice_download_voices') return downloadImpl();
  if (command === 'practice_preview_voice') return null;
  if (command === 'practice_new_questions') return null;
  if (command === 'practice_cancel_start') return null;
  if (command === 'practice_start') return startImpl();
  // The report tests open s1; a call (session-1, or run-1 on resume) reads the running detail
  if (command === 'practice_get_session') return structuredClone(args?.id === 's1' ? detail : running);
  if (command === 'practice_retry_report') return retryImpl();
  if (command === 'practice_speak_word') return speakImpl();
  throw new Error(`Unexpected command: ${command} ${JSON.stringify(args)}`);
});
const toast = { success: mock((_what: string) => {}), error: mock((_what: string, _opts?: unknown) => {}), info: mock(() => {}), warning: mock(() => {}) };

const listeners = new Map<string, (e: { payload: unknown }) => void>();
// When set, listen() for practice-prep waits on it, so a test can cancel while the listener registers
let holdPrepListen: Promise<void> | null = null;
// Order of listen() and invoke() calls, so a test can check one happened before the other
const trace: string[] = [];
mock.module('@tauri-apps/api/core', () => ({ invoke, convertFileSrc: (path: string) => `asset://${path}` }));
mock.module('@tauri-apps/api/event', () => ({
  ...originals.event,
  listen: async (event: string, handler: (e: { payload: unknown }) => void) => {
    trace.push(`listen:${event}`);
    if (event === 'practice-prep' && holdPrepListen) await holdPrepListen;
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

// True when a node has this text as a direct child, so a row is not matched through its parents
function hasDirectText(value: string): boolean {
  return renderer!.root.findAll((n) => n.children.some((c) => c === value)).length > 0;
}

async function openReport() {
  await clickButton('Open report');
  await flush();
}

beforeEach(async () => {
  status = { ...READY };
  setups = structuredClone([THESIS, INTERVIEW]);
  sessions = structuredClone([SESSION]);
  detail = structuredClone(DETAIL);
  running = structuredClone(RUNNING_DETAIL) as SessionDetail;
  startImpl = async () => 'session-1';
  retryImpl = async () => null;
  downloadImpl = async () => null;
  speakImpl = async () => null;
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

  test('start_is_re_read_on_focus_and_every_5_s_while_the_list_shows', async () => {
    const realSetInterval = window.setInterval;
    let tick: (() => void) | undefined;
    let every: number | undefined;
    window.setInterval = ((fn: () => void, ms?: number) => {
      tick = fn;
      every = ms;
      return 1;
    }) as typeof window.setInterval;
    try {
      status = { ...READY, blocked: 'Practice is unavailable while a meeting is recording, importing or re-transcribing' };
      await mount();
    } finally {
      window.setInterval = realSetInterval;
    }
    expect(every).toBe(5000);
    expect(buttons('▶ Start').every((b) => b.props.disabled)).toBe(true);

    status = { ...READY };
    await act(async () => tick!());
    await flush();
    expect(buttons('▶ Start').some((b) => !b.props.disabled)).toBe(true);

    status = { ...READY, blocked: 'Practice is unavailable while a meeting is recording, importing or re-transcribing' };
    await act(async () => {
      window.dispatchEvent(new Event('focus'));
    });
    await flush();
    expect(buttons('▶ Start').every((b) => b.props.disabled)).toBe(true);
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

  test('a cancel while the listener registers means nothing starts', async () => {
    let release!: () => void;
    holdPrepListen = new Promise<void>((resolve) => {
      release = resolve;
    });
    await clickButton('▶ Start');
    await clickButton('Cancel');
    await act(async () => release());
    await flush();
    holdPrepListen = null;
    expect(calls('practice_start').length).toBe(0);
    expect(listeners.has('practice-prep')).toBe(false);
    expect(renderer!.root.findAll((n) => n.props.role === 'dialog').length).toBe(0);
  });

  test('a session under 30 s lists as less than a minute', async () => {
    sessions = [{ ...SESSION, duration_secs: 20 }];
    await mount();
    const lines = renderer!.root.findAll((n) => n.type === 'p').map(text);
    expect(lines.some((l) => l.includes('<1 min'))).toBe(true);
    expect(lines.some((l) => l.includes('0 min'))).toBe(false);
  });

  test('new_questions_confirms_and_refreshes_the_setup', async () => {
    await clickButton('Edit');
    const listsBefore = calls('practice_list_setups').length;
    await clickButton('New questions');
    await flush();
    expect(calls('practice_new_questions')[0][1]).toEqual({ setupId: 'seed-thesis' });
    expect(toast.success.mock.calls.at(-1)?.[0]).toBe('New questions will be prepared at the next start');
    expect(calls('practice_list_setups').length).toBeGreaterThan(listsBefore);
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

  test('preview_before_the_first_session_downloads_the_voices_first', async () => {
    status = { ...READY, voices_ready: false };
    await mount();
    let release!: () => void;
    downloadImpl = () => new Promise((resolve) => {
      release = () => resolve(null);
    });
    await clickButton('+ New setup');
    await clickButton('▶ Preview');
    await act(async () => {
      listeners.get('practice-voices-progress')?.({ payload: { percent: 40 } });
    });
    expect(hasDirectText('Downloading voices… 40%')).toBe(true);
    expect(calls('practice_preview_voice').length).toBe(0);

    await act(async () => release());
    await flush();
    expect(calls('practice_download_voices').length).toBe(1);
    expect(calls('practice_preview_voice')[0][1]).toEqual({ voice: VOICES[0].id });
    expect(hasDirectText('Downloading voices… 40%')).toBe(false);
  });

  test('preview_with_voices_ready_does_not_download', async () => {
    await mount();
    await clickButton('+ New setup');
    await clickButton('▶ Preview');
    expect(calls('practice_download_voices').length).toBe(0);
    expect(calls('practice_preview_voice').length).toBe(1);
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

  test('a resumed session shows the turns said so far', async () => {
    running = { ...running, turns: TURNS };
    status = { ...READY, running_session: 'run-1' };
    await mount();
    await clickButton('Show full transcript ▾');
    expect(hasDirectText('Tell me about your thesis.')).toBe(true);
    expect(hasDirectText('If I will have more data')).toBe(true);
  });

  test('a resumed session that already ended opens its report', async () => {
    running = { ...running, summary: { ...running.summary, end_reason: 'user' } };
    status = { ...READY, running_session: 'run-1' };
    await mount();
    expect(tapButtons().length).toBe(0);
    expect(renderer!.root.findAll((n) => n.type === 'h1' && text(n).startsWith('Thesis defence · ')).length).toBe(1);
  });

  test('a resumed session is listened to before it is read', async () => {
    status = { ...READY, running_session: 'run-1' };
    trace.length = 0;
    await mount();
    const listened = trace.indexOf('listen:practice-turn');
    expect(listened).toBeGreaterThanOrEqual(0);
    expect(listened).toBeLessThan(trace.lastIndexOf('invoke:practice_get_session'));
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
    await flush();
    expect(tapButtons().length).toBe(0);
    expect(renderer!.root.findAll((n) => n.type === 'h1' && text(n).startsWith('Thesis defence · ')).length).toBe(1);
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

describe('Practice report view', () => {
  test('stats_strip_and_cards_render_counts', async () => {
    await openReport();
    expect(hasDirectText('31')).toBe(true);
    expect(hasDirectText('112')).toBe(true);
    expect(hasDirectText('6')).toBe(true);
    expect(hasDirectText('fillers')).toBe(true);
    expect(hasDirectText('words/min')).toBe(true);
    expect(hasDirectText('long pauses')).toBe(true);

    const headings = renderer!.root.findAll((n) => n.type === 'h3').map(text);
    expect(headings).toEqual(['Grammar (2)', 'Sounds unnatural (1)', 'Could be clearer (1)', 'Fillers and fluency']);
    expect(hasDirectText('um 14 · so basically 9 · like 8')).toBe(true);
  });

  test('grammar_quote_strikes_only_the_changed_words', async () => {
    await openReport();
    const struck = renderer!.root.findAll((n) => String(n.props.className).includes('line-through'));
    expect(struck.map(text)).toEqual(['will have', 'depend']);
    const better = renderer!.root.findAll((n) => String(n.props.className).includes('text-emerald-500'));
    expect(better.map(text)).toContain('If I had more data…');
  });

  test('echoed_badge_only_when_echoed', async () => {
    await openReport();
    const badges = renderer!.root.findAll((n) => n.children.some((c) => c === 'Prof. Weber echoed this'));
    expect(badges.length).toBe(1);
  });

  test('play_button_seeks_to_turn_offset', async () => {
    const audio = { currentTime: 0, play: mock(async () => {}), pause: mock(() => {}) };
    renderer?.unmount();
    renderer = null;
    await act(async () => {
      renderer = create(<PracticePage />, { createNodeMock: (el) => (el.type === 'audio' ? audio : null) });
    });
    await flush();
    await openReport();

    expect(renderer!.root.findAll((n) => n.type === 'audio')[0].props.src).toBe('asset://C:/Conclave/practice/s1/you.wav');
    await clickButton('▶ 06:12');
    expect(audio.currentTime).toBe(10);
    expect(audio.play).toHaveBeenCalledTimes(1);

    // The clip is 2 s long (32000 samples at 16 kHz): it pauses once the position passes 12 s
    const onTimeUpdate = renderer!.root.findAll((n) => n.type === 'audio')[0].props.onTimeUpdate;
    audio.currentTime = 11.5;
    await act(async () => onTimeUpdate());
    expect(audio.pause).not.toHaveBeenCalled();
    audio.currentTime = 12.1;
    await act(async () => onTimeUpdate());
    expect(audio.pause).toHaveBeenCalledTimes(1);
  });

  test('failed_report_offers_retry', async () => {
    detail = { ...detail, summary: { ...detail.summary, report_status: 'failed' }, report: null, report_error: 'Claude did not answer' };
    await openReport();
    expect(hasDirectText('Claude did not answer')).toBe(true);
    expect(buttons('Retry').length).toBe(1);

    await clickButton('Retry');
    await flush();
    expect(calls('practice_retry_report')[0][1]).toEqual({ sessionId: 's1' });
  });

  test('meeting_end_reason_shows_the_banner', async () => {
    detail = { ...detail, summary: { ...detail.summary, end_reason: 'meeting' } };
    await openReport();
    expect(hasDirectText('A meeting recording started, so the session ended')).toBe(true);
  });

  test('transcript_prefers_verbatim_text', async () => {
    await openReport();
    await clickButton('Transcript');
    expect(hasDirectText('Um, if I will have more data')).toBe(true);
    expect(hasDirectText('If I will have more data')).toBe(false);
    expect(hasDirectText('Tell me about your thesis.')).toBe(true);
  });

  test('pending_report_offers_generate', async () => {
    detail = { ...detail, summary: { ...detail.summary, report_status: 'pending' }, report: null };
    await openReport();
    expect(hasDirectText('Report not generated yet')).toBe(true);
    await clickButton('Generate report');
    await flush();
    expect(calls('practice_retry_report')[0][1]).toEqual({ sessionId: 's1' });
  });

  test('ready_without_a_report_offers_generate', async () => {
    detail = { ...detail, report: null };
    await openReport();
    const line = renderer!.root.findAll((n) => n.type === 'p' && text(n) === 'The report is missing — Generate report');
    expect(line.length).toBe(1);
    expect(buttons('Generate report').length).toBe(1);
  });

  test('failed_without_an_error_says_so', async () => {
    detail = { ...detail, summary: { ...detail.summary, report_status: 'failed' }, report: null, report_error: null };
    await openReport();
    expect(hasDirectText('The report could not be written')).toBe(true);
  });

  test('retry_is_disabled_while_the_call_is_in_flight', async () => {
    detail = { ...detail, summary: { ...detail.summary, report_status: 'failed' }, report: null, report_error: 'Claude did not answer' };
    await openReport();
    let release!: () => void;
    retryImpl = () => new Promise((resolve) => {
      release = () => resolve(null);
    });
    await clickButton('Retry');
    expect(buttons('Retry')[0].props.disabled).toBe(true);
    // A second click before the first call returns must not invoke again
    await clickButton('Retry');
    expect(calls('practice_retry_report').length).toBe(1);

    await act(async () => release());
    await flush();
    expect(buttons('Retry')[0].props.disabled).toBe(false);
  });

  test('empty_filler_breakdown_says_so', async () => {
    detail = { ...detail, report: { ...REPORT, fluency: { ...REPORT.fluency, fillers: {}, filler_total: 0 } } };
    await openReport();
    expect(hasDirectText('No fillers counted')).toBe(true);
  });

  test('short_session_shows_less_than_a_minute', async () => {
    detail = { ...detail, summary: { ...detail.summary, duration_secs: 20 } };
    await openReport();
    const heading = renderer!.root.findAll((n) => n.type === 'h1').map(text);
    expect(heading.length).toBe(1);
    expect(heading[0].endsWith('· <1 min')).toBe(true);
  });

  test('a session still finishing says so, without a Generate button', async () => {
    detail = { ...detail, summary: { ...detail.summary, report_status: 'pending', end_reason: null }, report: null };
    await openReport();
    expect(hasDirectText('Finishing the session…')).toBe(true);
    expect(buttons('Generate report').length).toBe(0);
  });

  test('the report that lands while the session finishes replaces the wait with Generate', async () => {
    detail = { ...detail, summary: { ...detail.summary, report_status: 'pending', end_reason: null }, report: null };
    await openReport();
    detail = { ...detail, summary: { ...detail.summary, report_status: 'pending', end_reason: 'user' }, report: null };
    await emit('practice-report', { session_id: 's1', status: 'pending' });
    await flush();
    expect(hasDirectText('Finishing the session…')).toBe(false);
    expect(buttons('Generate report').length).toBe(1);
  });

  test('the report listener is registered before the session is loaded', async () => {
    trace.length = 0;
    await openReport();
    const listened = trace.indexOf('listen:practice-report');
    expect(listened).toBeGreaterThanOrEqual(0);
    expect(listened).toBeLessThan(trace.indexOf('invoke:practice_get_session'));
  });

  test('struck_quote_has_no_stray_spaces', async () => {
    await openReport();
    const quotes = renderer!.root.findAll((n) => n.type === 'p' && text(n).startsWith('“')).map(text);
    expect(quotes).toContain('“If I will have more data…”');
    expect(quotes).toContain('“The data depend”');
  });

  test('pron_card_plays_word_span_and_reference', async () => {
    const audio = { currentTime: 0, play: mock(async () => {}), pause: mock(() => {}) };
    renderer?.unmount();
    renderer = null;
    await act(async () => {
      renderer = create(<PracticePage />, { createNodeMock: (el) => (el.type === 'audio' ? audio : null) });
    });
    await flush();
    detail = { ...detail, report: { ...REPORT, version: 2, pronunciation: [{ turn: 3, word: 'will', start_secs: 1.5, end_secs: 1.9, score: -7.2, phone: 'IH' }] } };
    await openReport();

    // The turn starts at 160_000 samples (10 s) in you.wav; the hint is 1.5 s to 1.9 s into the turn
    await clickButton('▶ you');
    expect(audio.currentTime).toBe(11.5);
    expect(audio.play).toHaveBeenCalledTimes(1);
    const onTimeUpdate = renderer!.root.findAll((n) => n.type === 'audio')[0].props.onTimeUpdate;
    audio.currentTime = 11.8;
    await act(async () => onTimeUpdate());
    expect(audio.pause).not.toHaveBeenCalled();
    audio.currentTime = 11.95;
    await act(async () => onTimeUpdate());
    expect(audio.pause).toHaveBeenCalledTimes(1);

    await clickButton('▶ reference');
    expect(calls('practice_speak_word')[0][1]).toEqual({ sessionId: 's1', word: 'will' });
    expect(hasDirectText('will')).toBe(true);
  });

  test('pron_card_shows_note_or_empty_text', async () => {
    detail = { ...detail, report: { ...REPORT, version: 2, pronunciation_note: 'Pronunciation hints unavailable' } };
    await openReport();
    expect(renderer!.root.findAll((n) => n.type === 'h3').map(text)).toContain('Pronunciation hints (may be off)');
    expect(hasDirectText('Pronunciation hints unavailable')).toBe(true);
    expect(hasDirectText('No pronunciation hints this time')).toBe(false);

    await clickButton('← Practice');
    detail = { ...detail, report: { ...REPORT, version: 2 } };
    await openReport();
    expect(hasDirectText('No pronunciation hints this time')).toBe(true);
  });

  test('pron_you_is_hidden_without_a_clip', async () => {
    detail = { ...detail, audio_path: null, report: { ...REPORT, version: 2, pronunciation: [HINT] } };
    await openReport();
    expect(buttons('▶ you').length).toBe(0);
    expect(buttons('▶ reference').length).toBe(1);
  });

  test('reference_ignores_clicks_while_a_speak_call_is_pending', async () => {
    detail = { ...detail, report: { ...REPORT, version: 2, pronunciation: [HINT] } };
    await openReport();
    let release!: () => void;
    speakImpl = () => new Promise((resolve) => {
      release = () => resolve(null);
    });
    await clickButton('▶ reference');
    await clickButton('▶ reference');
    expect(calls('practice_speak_word').length).toBe(1);

    await act(async () => release());
    await flush();
    await clickButton('▶ reference');
    expect(calls('practice_speak_word').length).toBe(2);
  });

  test('recurring_card_lists_patterns_and_trend', async () => {
    detail = {
      ...detail,
      report: { ...REPORT, version: 2 },
      recurring: {
        patterns: [{ key: 'make research', sessions: 3, same_setup: false }, { key: 'if + will', sessions: 4, same_setup: true }],
        filler_trend: [3.1, 2.4, 1.8],
      },
    };
    await openReport();
    expect(renderer!.root.findAll((n) => n.type === 'h3').map(text)).toContain('Keeps coming back');
    // Same-setup patterns come first
    const chips = renderer!.root.findAll((n) => n.type === 'span' && text(n).includes(' sessions')).map(text);
    expect(chips).toEqual(['if + will ×4 sessions', 'make research ×3 sessions']);
    expect(hasDirectText('Fillers per minute')).toBe(true);
    expect(hasDirectText('3.1 → 2.4 → 1.8')).toBe(true);

    // One trend point is not a trend, and no patterns means nothing to show: the card is hidden
    await clickButton('← Practice');
    detail = { ...detail, recurring: { patterns: [], filler_trend: [3.1] } };
    await openReport();
    expect(renderer!.root.findAll((n) => n.type === 'h3').map(text)).not.toContain('Keeps coming back');
  });

  test('v1_report_hides_new_cards', async () => {
    // The backend fills recurring for any ready report, so a v1 report with earlier sessions has one
    detail = { ...detail, recurring: { patterns: [], filler_trend: [3.1, 2.4] } };
    await openReport();
    expect(renderer!.root.findAll((n) => n.type === 'h3').map(text)).toEqual(['Grammar (2)', 'Sounds unnatural (1)', 'Could be clearer (1)', 'Fillers and fluency']);
    expect(hasDirectText('Pronunciation hints (may be off)')).toBe(false);
    expect(hasDirectText('Keeps coming back')).toBe(false);
  });
});
