import { afterAll, beforeEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingRow } from '../../src/lib/meetings';

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
mock.module('sonner', () => ({ toast: { error() {}, success() {}, warning() {}, info() {} } }));
mock.module('../../src/contexts/ConfigContext', () => ({
  useConfig: () => ({
    selectedDevices: { micDevice: 'mic', systemDevice: 'sys' }, setSelectedDevices() {},
    selectedLanguage: 'auto', setSelectedLanguage() {}, transcriptModelConfig: { provider: 'parakeet' },
  }),
}));
mock.module('../../src/contexts/RecordingStateContext', () => ({
  ...originals.recording,
  useRecordingState: () => ({ micFollowing: { warning: null } }),
}));
mock.module('../../src/components/DeviceSelection', () => ({
  DeviceSelection: () => <div><span>Microphone</span><span>System audio</span></div>,
}));
let languageProvider: unknown;
mock.module('../../src/components/LanguageSelection', () => ({
  LanguageSelection: (p: { provider?: string }) => { languageProvider = p.provider; return <span>Language</span>; },
}));

const { NewMeetingView } = await import('../../src/components/MeetingPage/NewMeetingView');
const { MeetingHeader } = await import('../../src/components/MeetingPage/MeetingHeader');

const row: MeetingRow = {
  id: 'm1', title: 'Interview', titleSource: 'user', templateId: 'interview', emoji: '🤝', reviewed: false, gist: null,
  startedAt: null, endedAt: null, createdAt: '2026-11-16 10:00:00+00:00', calendarUid: null,
  calendarStart: null, calendarEnd: null, calendarCancelledAt: null, hasTranscript: false, hasSummary: false, prepPoints: 0,
};
const text = (r: ReactTestRenderer) => JSON.stringify(r.toJSON());
beforeEach(() => { calls.length = 0; });

async function mountNew(props: Partial<Parameters<typeof NewMeetingView>[0]> = {}) {
  let r!: ReactTestRenderer;
  await act(async () => { r = create(<NewMeetingView row={row} live={null} disabled={false} onStart={() => {}} {...props} />); });
  return r;
}

test('a new meeting shows Prep notes, the Audio line and Start recording', async () => {
  const r = await mountNew();
  const t = text(r);
  for (const label of ['Prep notes', 'Microphone', 'System audio', 'Language', 'Start recording', 'Recording goes into this meeting.']) {
    expect(t).toContain(label);
  }
  expect(languageProvider).toBe('parakeet');
});

test('Start recording calls onStart', async () => {
  let started = 0;
  const r = await mountNew({ onStart: () => { started++; } });
  const button = r.root.findAllByType('button').find((b: ReactTestInstance) => JSON.stringify(b.children).includes('Start recording'))!;
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
  const button = r.root.findAllByType('button').find((b: ReactTestInstance) => JSON.stringify(b.children).includes('Start recording'))!;
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
