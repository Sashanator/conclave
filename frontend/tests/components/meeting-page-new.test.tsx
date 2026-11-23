import { afterAll, beforeEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingRow } from '../../src/lib/meetings';

let provider = 'localWhisper';
const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  sonner: { ...await import('sonner') },
  config: { ...await import('../../src/contexts/ConfigContext') },
  recording: { ...await import('../../src/contexts/RecordingStateContext') },
  device: { ...await import('../../src/components/DeviceSelection') },
  language: { ...await import('../../src/components/LanguageSelection') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/contexts/ConfigContext', () => originals.config);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.recording);
  mock.module('../../src/components/DeviceSelection', () => originals.device);
  mock.module('../../src/components/LanguageSelection', () => originals.language);
});

const calls: Array<[string, unknown]> = [];
let prepResult: () => Promise<unknown> = async () => null;
const invoke = mock(async (command: string, args?: unknown) => {
  calls.push([command, args]);
  if (command === 'prep_for_meeting') return prepResult();
  if (command === 'prep_save') return { id: 'p1', eventUid: null, eventStart: null, eventTitle: 'Interview', material: (args as { material?: string })?.material ?? '', items: [] };
  if (command === 'api_list_templates') return { templates: [{ id: 'interview', name: 'Interview', emoji: '🤝' }], errors: [] };
  return null;
});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
let toastErrors = 0;
mock.module('sonner', () => ({ toast: { error() { toastErrors++; }, success() {}, warning() {}, info() {} } }));
mock.module('../../src/contexts/ConfigContext', () => ({
  useConfig: () => ({
    selectedDevices: { micDevice: 'mic', systemDevice: 'sys' }, setSelectedDevices() {},
    selectedLanguage: 'auto', setSelectedLanguage() {}, allowedLanguages: ['ru', 'en'], setAllowedLanguages() {}, transcriptModelConfig: { provider },
  }),
}));
mock.module('../../src/contexts/RecordingStateContext', () => ({
  ...originals.recording,
  useRecordingState: () => ({ micFollowing: { warning: null } }),
}));
mock.module('../../src/components/DeviceSelection', () => ({
  DeviceSelection: () => <div><span>Microphone</span><span>System audio</span></div>,
}));
mock.module('../../src/components/LanguageSelection', () => ({ LanguageSelection: () => <span>Language</span> }));

const { NewMeetingView } = await import('../../src/components/MeetingPage/NewMeetingView');
const { MeetingHeader } = await import('../../src/components/MeetingPage/MeetingHeader');

const row: MeetingRow = {
  id: 'm1', title: 'Interview', titleSource: 'user', templateId: 'interview', emoji: '🤝', reviewed: false, gist: null,
  startedAt: null, endedAt: null, createdAt: '2026-11-16 10:00:00+00:00', calendarUid: null,
  calendarStart: null, calendarEnd: null, calendarCancelledAt: null, hasTranscript: false, hasSummary: false, prepPoints: 0, hasPrep: false,
};
const plain = (n: ReactTestInstance | string): string => (typeof n === 'string' ? n : n.children.map(plain).join(''));
const text = (r: ReactTestRenderer) => JSON.stringify(r.toJSON());
beforeEach(() => { provider = 'localWhisper'; calls.length = 0; toastErrors = 0; });

async function mountNew(props: Partial<Parameters<typeof NewMeetingView>[0]> = {}) {
  let r!: ReactTestRenderer;
  await act(async () => { r = create(<NewMeetingView row={row} live={null} disabled={false} onStart={() => {}} header={<MeetingHeader variant="new" row={row} onChange={() => {}} />} {...props} />); });
  return r;
}

test('a new meeting shows Start, the audio line, languages and prep material', async () => {
  const r = await mountNew();
  const t = text(r);
  for (const label of ['Prep material and chat', 'Mic:', 'Start recording', 'Recording goes into this meeting.']) expect(t).toContain(label);
  for (const gone of ['Test Mic', 'Auto Detect may produce']) expect(t).not.toContain(gone);
});

test('the form and prep columns have a keyboard-resizable handle', async () => {
  const r = await mountNew();
  const sep = r.root.findByProps({ role: 'separator' });
  expect(sep.props['aria-valuenow']).toBe(50);
  await act(async () => { sep.props.onKeyDown({ key: 'ArrowRight', preventDefault() {} }); });
  expect(r.root.findByProps({ role: 'separator' }).props['aria-valuenow']).toBe(55);
});

test('Start recording comes before the audio line, languages and prep', async () => {
  const t = text(await mountNew());
  const at = (x: string) => t.indexOf(x);
  expect(at('Start recording')).toBeLessThan(at('Mic:'));
  expect(at('Mic:')).toBeLessThan(at('Languages spoken'));
  expect(at('Languages spoken')).toBeLessThan(at('Prep material'));
});

const defaultRow: MeetingRow = { ...row, titleSource: 'default', title: 'New meeting 16 Nov' };
const mountHeader = async (r: MeetingRow) => {
  let t!: ReactTestRenderer;
  await act(async () => { t = create(<MeetingHeader variant="new" row={r} onChange={() => {}} />); });
  return t;
};

test('a default title shows an empty labelled Meeting name', async () => {
  const h = await mountHeader(defaultRow);
  const input = h.root.findByType('input');
  expect(input.props.value).toBe('');
  expect(input.props.placeholder).toBe('New meeting 16 Nov');
  expect(h.root.findByType('label').props.htmlFor).toBe(input.props.id);
  expect(JSON.stringify(h.toJSON())).toContain('Meeting name');
  await act(async () => { await input.props.onBlur(); });
  expect(calls.filter(([c]) => c === 'api_save_meeting_title')).toEqual([]);
  expect(toastErrors).toBe(0);
});

test('a calendar title is shown', async () => {
  const h = await mountHeader({ ...row, titleSource: 'calendar', title: 'Weekly sync' });
  expect(h.root.findByType('input').props.value).toBe('Weekly sync');
});

test('Start recording calls onStart', async () => {
  let started = 0;
  const r = await mountNew({ onStart: () => { started++; } });
  const button = r.root.findAllByType('button').find((b: ReactTestInstance) => plain(b).includes('Start recording'))!;
  await act(async () => { button.props.onClick(); });
  expect(started).toBe(1);
});

test('while another meeting is live, its title and Open replace the button', async () => {
  const r = await mountNew({ live: { id: 'm9', title: 'Standup' } });
  const t = text(r);
  expect(t).toContain('Standup');
  expect(t).toContain('Open');
  expect(t).not.toContain('Start recording');
});

test('a disabled start (saving) keeps the button inert', async () => {
  const r = await mountNew({ disabled: true });
  const button = r.root.findAllByType('button').find((b: ReactTestInstance) => plain(b).includes('Start recording'))!;
  expect(button.props.disabled).toBe(true);
});

test('the prep notes save on blur with the meeting id', async () => {
  const r = await mountNew();
  const area = r.root.findByType('textarea');
  await act(async () => { area.props.onChange({ target: { value: 'Ask about budget' } }); });
  await act(async () => { await r.root.findByType('textarea').props.onBlur(); });
  const save = calls.find(([c]) => c === 'prep_save')!;
  expect((save[1] as { meetingId: string; material: string }).meetingId).toBe('m1');
  expect((save[1] as { material: string }).material).toBe('Ask about budget');
});

const saves = () => calls.filter(([c]) => c === 'prep_save');
const sleep = (ms: number) => act(async () => { await new Promise((r) => setTimeout(r, ms)); });

test('typing saves the prep notes after a pause, without a blur', async () => {
  const r = await mountNew();
  await act(async () => { r.root.findByType('textarea').props.onChange({ target: { value: 'Ask about budget' } }); });
  expect(saves().length).toBe(0);
  await sleep(900);
  expect(saves().length).toBe(1);
  expect((saves()[0][1] as { material: string }).material).toBe('Ask about budget');
});

test('a blur after the pause save does not save the same text twice', async () => {
  const r = await mountNew();
  await act(async () => { r.root.findByType('textarea').props.onChange({ target: { value: 'Ask' } }); });
  await sleep(900);
  await act(async () => { await r.root.findByType('textarea').props.onBlur(); });
  expect(saves().length).toBe(1);
});

test('leaving the page saves notes typed a moment ago', async () => {
  const r = await mountNew();
  await act(async () => { r.root.findByType('textarea').props.onChange({ target: { value: 'Last words' } }); });
  await act(async () => { r.unmount(); });
  await sleep(10);
  expect(saves().length).toBe(1);
  expect((saves()[0][1] as { material: string }).material).toBe('Last words');
});

test('leaving with nothing typed saves no empty prep', async () => {
  const r = await mountNew();
  await act(async () => { r.unmount(); });
  await sleep(10);
  expect(saves().length).toBe(0);
});

test('the emoji precedes the title input', async () => {
  let r!: ReactTestRenderer;
  await act(async () => { r = create(<MeetingHeader row={row} onChange={() => {}} />); });
  const t = text(r);
  expect(t.indexOf('🤝')).toBeGreaterThan(-1);
  expect(t.indexOf('🤝')).toBeLessThan(t.indexOf('Meeting title'));
});

const calendarPrep = { id: 5, eventUid: 'ev1', eventStart: '2026-11-17T10:00:00Z', eventTitle: 'Standup', templateId: null, material: 'Old', chat: [], approvedAt: null, createdAt: '', updatedAt: '', items: [] };

test('the prep notes are read-only until the load answered, and a failed load never saves', async () => {
  prepResult = async () => { throw new Error('read failed'); };
  const r = await mountNew();
  expect(r.root.findByType('textarea').props.readOnly).toBe(true);
  calls.length = 0;
  await act(async () => { await r.root.findByType('textarea').props.onBlur(); });
  expect(calls.filter(([c]) => c === 'prep_save')).toEqual([]);
  prepResult = async () => null;
});

test('a loaded calendar prep keeps its event when the notes are saved', async () => {
  prepResult = async () => calendarPrep;
  const r = await mountNew();
  const area = r.root.findByType('textarea');
  expect(area.props.readOnly).toBe(false);
  expect(area.props.value).toBe('Old');
  await act(async () => { area.props.onChange({ target: { value: 'New text' } }); });
  await act(async () => { await r.root.findByType('textarea').props.onBlur(); });
  const save = calls.find(([c]) => c === 'prep_save')![1] as Record<string, unknown>;
  expect(save.eventUid).toBe('ev1');
  expect(save.eventStart).toBe('2026-11-17T10:00:00Z');
  prepResult = async () => null;
});

test('a slow load finishing after the page changed does not overwrite the notes', async () => {
  let finish!: (p: unknown) => void;
  prepResult = () => new Promise((done) => { finish = done; });
  const r = await mountNew();
  expect(r.root.findByType('textarea').props.readOnly).toBe(true);
  await act(async () => { finish(calendarPrep); });
  expect(r.root.findByType('textarea').props.value).toBe('Old');
  prepResult = async () => null;
});

test('the header reports a saved title', async () => {
  let saved = 0;
  let r!: ReactTestRenderer;
  await act(async () => { r = create(<MeetingHeader row={row} onChange={() => {}} onTitleSaved={() => { saved++; }} />); });
  const input = r.root.findByProps({ 'aria-label': 'Meeting title' });
  await act(async () => { input.props.onChange({ target: { value: 'Renamed' } }); });
  await act(async () => { await r.root.findByProps({ 'aria-label': 'Meeting title' }).props.onBlur(); });
  expect(saved).toBe(1);
});

test('Parakeet hides Languages spoken, as Settings does', async () => {
  provider = 'parakeet';
  const t = text(await mountNew());
  expect(t).not.toContain('Languages spoken');
  expect(t).toContain('Start recording');
});
