import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import {
  MATERIAL_MAX_BYTES, type PracticeStatus, type Report, type SessionDetail, type SessionSummary, type SetupSummary, type TurnRow,
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
const READY: PracticeStatus = { has_claude_key: true, engine_is_whisper: true, voices_ready: true, blocked: null, running_session: null };
const REPORT: Report = {
  version: 1,
  fluency: { fillers: { um: 14, 'so basically': 9, like: 8 }, filler_total: 31, words: 1200, speech_secs: 640, wpm: 112.4, long_pauses: 6 },
  items: [
    { kind: 'Grammar', turn: 3, quote: 'If I will have more data…', better: 'If I had more data…', note: 'Past simple after "if".', echoed: true },
    { kind: 'Grammar', turn: 5, quote: 'The data depend', better: 'The data depends', note: 'Third person singular.', echoed: false },
    { kind: 'Unnatural', turn: 7, quote: 'I made a research about it', better: 'I did some research on it', note: 'Native speakers "do" research.', echoed: false },
    { kind: 'Clarity', turn: 9, quote: 'So basically the idea was that, well, the baseline', better: 'The baseline missed long-range links.', note: 'Shorter.', echoed: false },
  ],
  dropped_items: 0,
};
const TURNS: TurnRow[] = [
  { session_id: 's1', idx: 0, speaker: 'character', live_text: 'Tell me about your thesis.', verbatim_text: null, at_secs: 0, wav_offset: null, wav_len: null },
  { session_id: 's1', idx: 3, speaker: 'user', live_text: 'If I will have more data', verbatim_text: 'Um, if I will have more data', at_secs: 372, wav_offset: 160_000, wav_len: 32_000 },
];
const DETAIL: SessionDetail = {
  summary: { ...SESSION, report_status: 'ready' }, character: 'Prof. Weber, strict examiner', turns: TURNS,
  report: REPORT, report_error: null, audio_path: 'C:/Conclave/practice/s1/you.wav',
};

let status: PracticeStatus;
let setups: SetupSummary[];
let sessions: SessionSummary[];
let detail: SessionDetail;
// What practice_start does in the next call; tests replace it
let startImpl: () => Promise<unknown> = async () => 'session-1';
// Starts left pending by holdStarts(), settled by the test in any order
let pendingStarts: { resolve: (id: string) => void; reject: (err: Error) => void }[] = [];
// What practice_retry_report does; a test replaces it to hold the call open
let retryImpl: () => Promise<unknown> = async () => null;
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
  if (command === 'practice_get_session') return structuredClone(detail);
  if (command === 'practice_retry_report') return retryImpl();
  throw new Error(`Unexpected command: ${command} ${JSON.stringify(args)}`);
});
const toast = { success: mock(() => {}), error: mock((_what: string, _opts?: unknown) => {}), info: mock(() => {}), warning: mock(() => {}) };

const listeners = new Map<string, (e: { payload: unknown }) => void>();
mock.module('@tauri-apps/api/core', () => ({ invoke, convertFileSrc: (path: string) => `asset://${path}` }));
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

// True when a node has this text as a direct child, so a row is not matched through its parents
function hasText(value: string): boolean {
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
  startImpl = async () => 'session-1';
  retryImpl = async () => null;
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

describe('Practice report view', () => {
  test('stats_strip_and_cards_render_counts', async () => {
    await openReport();
    expect(hasText('31')).toBe(true);
    expect(hasText('112')).toBe(true);
    expect(hasText('6')).toBe(true);
    expect(hasText('fillers')).toBe(true);
    expect(hasText('words/min')).toBe(true);
    expect(hasText('long pauses')).toBe(true);

    const headings = renderer!.root.findAll((n) => n.type === 'h3').map(text);
    expect(headings).toEqual(['Grammar (2)', 'Sounds unnatural (1)', 'Could be clearer (1)', 'Fillers and fluency']);
    expect(hasText('um 14 · so basically 9 · like 8')).toBe(true);
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
    expect(hasText('Claude did not answer')).toBe(true);
    expect(buttons('Retry').length).toBe(1);

    await clickButton('Retry');
    await flush();
    expect(calls('practice_retry_report')[0][1]).toEqual({ sessionId: 's1' });
  });

  test('meeting_end_reason_shows_the_banner', async () => {
    detail = { ...detail, summary: { ...detail.summary, end_reason: 'meeting' } };
    await openReport();
    expect(hasText('A meeting recording started, so the session ended')).toBe(true);
  });

  test('transcript_prefers_verbatim_text', async () => {
    await openReport();
    await clickButton('Transcript');
    expect(hasText('Um, if I will have more data')).toBe(true);
    expect(hasText('If I will have more data')).toBe(false);
    expect(hasText('Tell me about your thesis.')).toBe(true);
  });

  test('pending_report_offers_generate', async () => {
    detail = { ...detail, summary: { ...detail.summary, report_status: 'pending' }, report: null };
    await openReport();
    expect(hasText('Report not generated yet')).toBe(true);
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
    expect(hasText('The report could not be written')).toBe(true);
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
    expect(hasText('No fillers counted')).toBe(true);
  });

  test('short_session_shows_less_than_a_minute', async () => {
    detail = { ...detail, summary: { ...detail.summary, duration_secs: 20 } };
    await openReport();
    const heading = renderer!.root.findAll((n) => n.type === 'h1').map(text);
    expect(heading.length).toBe(1);
    expect(heading[0].endsWith('· <1 min')).toBe(true);
  });

  test('struck_quote_has_no_stray_spaces', async () => {
    await openReport();
    const quotes = renderer!.root.findAll((n) => n.type === 'p' && text(n).startsWith('“')).map(text);
    expect(quotes).toContain('“If I will have more data…”');
    expect(quotes).toContain('“The data depend”');
  });
});
