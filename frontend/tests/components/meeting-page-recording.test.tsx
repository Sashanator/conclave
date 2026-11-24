import { afterAll, beforeEach, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingRow } from '../../src/lib/meetings';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  path: { ...await import('@tauri-apps/api/path') },
  sonner: { ...await import('sonner') },
  tooltip: { ...await import('../../src/components/ui/tooltip') },
  config: { ...await import('../../src/contexts/ConfigContext') },
  recording: { ...await import('../../src/contexts/RecordingStateContext') },
  transcripts: { ...await import('../../src/contexts/TranscriptContext') },
  sidebar: { ...await import('../../src/components/AiSidebar/AiSidebarProvider') },
  stop: { ...await import('../../src/hooks/useRecordingStop') },
  panel: { ...await import('../../src/app/_components/TranscriptPanel') },
  continueControl: { ...await import('../../src/app/_components/ContinueControl') },
  device: { ...await import('../../src/components/DeviceSelection') },
  language: { ...await import('../../src/components/LanguageSelection') },
  crumb: { ...await import('../../src/components/BackCrumb') },
  meter: { ...await import('../../src/components/AudioLevelMeter') },
  micPicker: { ...await import('../../src/components/MeetingPage/MicPicker') },
  silentMic: { ...await import('../../src/components/MeetingPage/SilentMicWarning') },
};
afterAll(() => {
  mock.module('../../src/components/MeetingPage/MicPicker', () => originals.micPicker);
  mock.module('../../src/components/MeetingPage/SilentMicWarning', () => originals.silentMic);
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('@tauri-apps/api/path', () => originals.path);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/components/ui/tooltip', () => originals.tooltip);
  mock.module('../../src/contexts/ConfigContext', () => originals.config);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.recording);
  mock.module('../../src/contexts/TranscriptContext', () => originals.transcripts);
  mock.module('../../src/components/AiSidebar/AiSidebarProvider', () => originals.sidebar);
  mock.module('../../src/hooks/useRecordingStop', () => originals.stop);
  mock.module('../../src/app/_components/TranscriptPanel', () => originals.panel);
  mock.module('../../src/app/_components/ContinueControl', () => originals.continueControl);
  mock.module('../../src/components/DeviceSelection', () => originals.device);
  mock.module('../../src/components/LanguageSelection', () => originals.language);
  mock.module('../../src/components/BackCrumb', () => originals.crumb);
  mock.module('../../src/components/AudioLevelMeter', () => originals.meter);
});

const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
const calls: string[] = [];
let stopFails = false;
const invoke = mock(async (command: string) => {
  calls.push(command);
  if (command === 'stop_recording' && stopFails) throw new Error('boom');
  if (command === 'api_list_templates') return { templates: [], errors: [] };
  if (command === 'prep_for_meeting') return null;
  return null;
});
const handleRecordingStop = mock(async (_callApi: boolean) => {});
const setIsStopping = mock((_v: boolean) => {});
const setStatus = mock((_s: string) => {});
const recording: Record<string, unknown> = {};
let sidebarOpen = false;

mock.module('@tauri-apps/api/core', () => ({ invoke }));
const handlers = new Map<string, (e: { payload: unknown }) => void>();
mock.module('@tauri-apps/api/event', () => ({ listen: async (name: string, h: (e: { payload: unknown }) => void) => { handlers.set(name, h); return () => { handlers.delete(name); }; } }));
const onBack = mock(() => {});
mock.module('../../src/components/BackCrumb', () => ({ BackCrumb: () => <button type="button" onClick={onBack}>Back</button> }));
const meters: Array<{ rmsLevel: number }> = [];
mock.module('../../src/components/AudioLevelMeter', () => ({ ...originals.meter, CompactAudioLevelMeter: (p: { rmsLevel: number }) => { meters.push(p); return null; } }));
mock.module('@tauri-apps/api/path', () => ({ appDataDir: async () => '/data' }));
mock.module('sonner', () => ({ toast: { error() {}, success() {}, warning() {}, info() {} } }));
mock.module('../../src/components/ui/tooltip', () => ({ Tooltip: passThrough, TooltipTrigger: passThrough, TooltipContent: passThrough, TooltipProvider: passThrough }));
mock.module('../../src/contexts/ConfigContext', () => ({
  useConfig: () => ({
    selectedDevices: { micDevice: 'mic', systemDevice: 'sys' }, setSelectedDevices() {},
    selectedLanguage: 'auto', setSelectedLanguage() {}, allowedLanguages: [], transcriptModelConfig: { provider: 'parakeet' },
  }),
}));
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals.recording, useRecordingState: () => ({ ...recording, setStatus }) }));
mock.module('../../src/contexts/TranscriptContext', () => ({
  ...originals.transcripts,
  useTranscripts: () => ({ notes: [], addNote() {}, updateNote() {}, deleteNote() {}, calendarLink: null }),
}));
mock.module('../../src/components/AiSidebar/AiSidebarProvider', () => ({ ...originals.sidebar, useAiSidebar: () => ({ open: sidebarOpen }) }));
mock.module('../../src/hooks/useRecordingStop', () => ({ useRecordingStop: () => ({ handleRecordingStop, setIsStopping }) }));
mock.module('../../src/app/_components/TranscriptPanel', () => ({ TranscriptPanel: () => <div>Live transcript</div> }));
mock.module('../../src/app/_components/ContinueControl', () => ({ ContinueControl: () => null }));
mock.module('../../src/components/MeetingPage/MicPicker', () => ({ MicPicker: () => <span>Mic picker</span>, useLiveMic: () => ['mic', () => {}] }));
mock.module('../../src/components/MeetingPage/SilentMicWarning', () => ({ SilentMicWarning: () => null }));
mock.module('../../src/components/DeviceSelection', () => ({ DeviceSelection: () => <span>Microphone</span> }));
mock.module('../../src/components/LanguageSelection', () => ({ LanguageSelection: () => <span>Language</span> }));

const { RecordingView } = await import('../../src/components/MeetingPage/RecordingView');

const row: MeetingRow = {
  id: 'm1', title: 'Weekly', titleSource: 'user', emoji: null, templateId: null, startedAt: '2026-11-17T10:00:00Z',
  createdAt: '2026-11-17T10:00:00Z', hasTranscript: false,
} as unknown as MeetingRow;
const text = (n: ReactTestInstance): string => n.children.map((c) => (typeof c === 'string' ? c : text(c))).join('');
const buttonWith = (r: ReactTestRenderer, label: string) => r.root.findAllByType('button').find((b) => text(b).includes(label));

const { meterLevel } = await import('../../src/components/MeetingPage/LiveLevels');

async function mount(): Promise<ReactTestRenderer> {
  let r!: ReactTestRenderer;
  await act(async () => { r = create(<RecordingView row={row} onChange={() => {}} showModal={() => {}} />); });
  return r;
}

beforeEach(() => {
  calls.length = 0;
  handleRecordingStop.mockClear(); setIsStopping.mockClear(); setStatus.mockClear();
  sidebarOpen = false;
  stopFails = false;
  Object.assign(recording, { micFollowing: { warning: null }, isRecording: true, isPaused: false, activeDuration: 73, status: 'recording', isStopping: false, isProcessing: false, isSaving: false, isStartingRecording: false });
});

test('the header shows the time, Pause, Stop and summarize and the audio line', async () => {
  const t = text((await mount()).root);
  for (const s of ['Recording 01:13', 'Pause', 'Stop and summarize', 'Live transcript', 'Mic:Mic picker', 'System audio: sys']) expect(t).toContain(s);
  expect(t).not.toContain('Microphone');
});

test('the header is one row', async () => {
  const r = await mount();
  const row = r.root.findByProps({ role: 'status' }).parent!;
  expect(text(row)).toContain('Mic');
  expect(text(row)).toContain('Stop and summarize');
});

test('levels come from audio-levels', async () => {
  meters.length = 0;
  await mount();
  await act(async () => { handlers.get('audio-levels')!({ payload: { levels: [{ device_type: 'input', rms_level: 0.5, peak_level: 0.5, is_active: true, device_name: 'microphone' }] } }); });
  expect(meters[meters.length - 2].rmsLevel).toBeCloseTo(meterLevel(0.5), 5); // Mic first, System second
  expect(meters[meters.length - 1].rmsLevel).toBe(0);
});

test('meterLevel maps dBFS: -60 is 0, 0 is 1, speech is visible', () => {
  expect(meterLevel(0)).toBe(0);
  expect(meterLevel(0.0005)).toBe(0);
  expect(meterLevel(1)).toBe(1);
  expect(meterLevel(2)).toBe(1);
  expect(meterLevel(0.05)).toBeCloseTo(0.567, 2);
});

test('leaving never stops the recording', async () => {
  calls.length = 0;
  const r = await mount();
  await act(async () => { r.unmount(); });
  expect(calls).not.toContain('stop_recording');
  expect(handleRecordingStop).not.toHaveBeenCalled();
});

test('sidebar open: Transcript and My notes are tabs; closed: no tablist', async () => {
  sidebarOpen = true;
  const open = await mount();
  expect(open.root.findAllByProps({ role: 'tab' }).map(text)).toEqual(['Transcript', 'My notes']);
  sidebarOpen = false;
  const closed = await mount();
  expect(closed.root.findAllByProps({ role: 'tab' })).toHaveLength(0);
  expect(text(closed.root)).toContain('My notes');
  expect(text(closed.root)).toContain('Live transcript');
});

test('Stop stops the backend once and hands the save to the stop hook', async () => {
  const r = await mount();
  await act(async () => { await buttonWith(r, 'Stop and summarize')!.props.onClick(); });
  expect(calls.filter((c) => c === 'stop_recording')).toHaveLength(1);
  expect(setIsStopping).toHaveBeenCalledWith(true);
  expect(handleRecordingStop).toHaveBeenCalledTimes(1);
  expect(handleRecordingStop).toHaveBeenCalledWith(true);
  expect(r.toJSON()).not.toBeNull();
});

test('a failed stop goes back to recording and never starts the save', async () => {
  stopFails = true;
  const r = await mount();
  await act(async () => { await buttonWith(r, 'Stop and summarize')!.props.onClick(); });
  expect(handleRecordingStop).not.toHaveBeenCalled();
  expect(setStatus).toHaveBeenCalledWith('recording');
});

test('a failed save says so, keeps the lines and offers a retry', async () => {
  Object.assign(recording, { isRecording: false, status: 'error', activeDuration: null });
  const r = await mount();
  expect(text(r.root)).toContain('The meeting was not saved');
  expect(text(r.root)).toContain('Live transcript');
  await act(async () => { buttonWith(r, 'Try saving again')!.props.onClick(); });
  expect(handleRecordingStop).toHaveBeenCalledWith(true);
});

test('after the stop the bar says Saving and shows no controls or red dot', async () => {
  Object.assign(recording, { isRecording: false, isStopping: false, isSaving: true, status: 'saving', activeDuration: null });
  const r = await mount();
  const t = text(r.root);
  expect(t).toContain('Saving');
  expect(t).not.toContain('Recording --:--');
  expect(buttonWith(r, 'Stop and summarize')).toBeUndefined();
  expect(r.root.findAll((n) => typeof n.props.style === 'object' && n.props.style?.backgroundColor === 'hsl(var(--rec))')).toHaveLength(0);
});

test('a failed save says Not saved, not Recording', async () => {
  Object.assign(recording, { isRecording: false, status: 'error', activeDuration: null });
  const t = text((await mount()).root);
  expect(t).toContain('Not saved');
  expect(t).not.toContain('Recording --:--');
});

test('while recording the live dot uses the rec token', async () => {
  const r = await mount();
  expect(r.root.findAll((n) => typeof n.props.style === 'object' && n.props.style?.backgroundColor === 'hsl(var(--rec))').length).toBeGreaterThan(0);
});
