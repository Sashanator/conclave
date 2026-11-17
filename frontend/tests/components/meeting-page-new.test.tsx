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
const invoke = mock(async (command: string, args?: unknown) => {
  calls.push([command, args]);
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

test('the emoji precedes the title input', async () => {
  let r!: ReactTestRenderer;
  await act(async () => { r = create(<MeetingHeader row={row} onChange={() => {}} />); });
  const t = text(r);
  expect(t.indexOf('🤝')).toBeGreaterThan(-1);
  expect(t.indexOf('🤝')).toBeLessThan(t.indexOf('Meeting title'));
});
