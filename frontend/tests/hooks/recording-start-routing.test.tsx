import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { UseRecordingStartReturn } from '../../src/hooks/useRecordingStart';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  sonner: { ...await import('sonner') },
  transcript: { ...await import('../../src/contexts/TranscriptContext') },
  sidebar: { ...await import('../../src/components/Sidebar/SidebarProvider') },
  config: { ...await import('../../src/contexts/ConfigContext') },
  state: { ...await import('../../src/contexts/RecordingStateContext') },
  recording: { ...await import('../../src/services/recordingService') },
  notification: { ...await import('../../src/lib/recordingNotification') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/contexts/TranscriptContext', () => originals.transcript);
  mock.module('../../src/components/Sidebar/SidebarProvider', () => originals.sidebar);
  mock.module('../../src/contexts/ConfigContext', () => originals.config);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.state);
  mock.module('../../src/services/recordingService', () => originals.recording);
  mock.module('../../src/lib/recordingNotification', () => originals.notification);
});

let routeResult: () => Promise<unknown> = async () => ({ meetingId: 'routed-1', title: 'Untitled · Mon 14:02' });
let startResult: () => Promise<void> = async () => {};
const calls: Array<[string, unknown]> = [];
const invoke = mock(async (command: string, args?: unknown) => {
  calls.push([command, args]);
  if (command === 'api_get_transcript_config') return { provider: 'parakeet' };
  if (command === 'parakeet_has_available_models') return true;
  if (command === 'api_route_recording_start') return routeResult();
  return undefined;
});
const toast = {
  success: mock((..._a: unknown[]) => {}), error: mock((..._a: unknown[]) => {}),
  warning: mock((..._a: unknown[]) => {}), info: mock((..._a: unknown[]) => {}),
};
const startRecordingWithDevices = mock(async (..._a: unknown[]) => startResult());
const setStartTitle = mock((_t: unknown) => {});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('sonner', () => ({ toast }));
mock.module('../../src/contexts/TranscriptContext', () => ({ useTranscripts: () => ({ clearTranscripts() {}, setStartTitle }) }));
mock.module('../../src/components/Sidebar/SidebarProvider', () => ({ useSidebar: () => ({ setIsMeetingActive() {} }) }));
mock.module('../../src/contexts/ConfigContext', () => ({ useConfig: () => ({ selectedDevices: { micDevice: 'mic', systemDevice: 'sys' } }) }));
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals.state, useRecordingState: () => ({ setStatus() {} }) }));
mock.module('../../src/services/recordingService', () => ({ recordingService: { startRecordingWithDevices } }));
mock.module('../../src/lib/recordingNotification', () => ({ showRecordingNotification: async () => {} }));
const { useRecordingStart } = await import('../../src/hooks/useRecordingStart');

let hook: UseRecordingStartReturn;
let renderer: ReactTestRenderer | undefined;
function View() { hook = useRecordingStart(false, () => {}); return null; }
async function mount() {
  await act(async () => { renderer = create(<View />); });
  await act(async () => { await new Promise((r) => setTimeout(r, 20)); });
}
const routeCalls = () => calls.filter(([c]) => c === 'api_route_recording_start');
const releaseCalls = () => calls.filter(([c]) => c === 'api_release_routed_start');

beforeEach(() => {
  calls.length = 0;
  invoke.mockClear(); startRecordingWithDevices.mockClear(); setStartTitle.mockClear();
  for (const t of Object.values(toast)) t.mockClear();
  routeResult = async () => ({ meetingId: 'routed-1', title: 'Untitled · Mon 14:02' });
  startResult = async () => {};
  sessionStorage.clear();
});
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
});

describe('manual start', () => {
  test('passes the routed meeting id and title to the recording command', async () => {
    await mount();
    await act(async () => { await hook.handleRecordingStart(); });
    expect(startRecordingWithDevices).toHaveBeenCalledWith('mic', 'sys', 'Untitled · Mon 14:02', 'routed-1');
  });

  test('a click event passed in as the argument is not taken for a meeting id', async () => {
    await mount();
    await act(async () => { await hook.handleRecordingStart({ type: 'click' }); });
    expect(routeCalls()[0][1]).toEqual({ source: 'sidebar' });
  });

  test('a string id routes to that meeting page', async () => {
    await mount();
    await act(async () => { await hook.handleRecordingStart('m-page'); });
    expect(routeCalls()[0][1]).toEqual({ source: { page: 'm-page' } });
  });

  test('a routing error still starts, with no target and no title', async () => {
    routeResult = async () => { throw 'This meeting no longer exists'; };
    await mount();
    await act(async () => { await hook.handleRecordingStart(); });
    expect(startRecordingWithDevices).toHaveBeenCalledWith('mic', 'sys', null, null);
    expect(toast.warning).toHaveBeenCalledWith('Recording as a new meeting', expect.anything());
  });

  test('a failed start releases the routed row', async () => {
    startResult = async () => { throw new Error('no microphone'); };
    const logged = console.error; console.error = () => {};
    await mount();
    await act(async () => { await hook.handleRecordingStart().catch(() => {}); });
    console.error = logged;
    expect(releaseCalls()).toEqual([['api_release_routed_start', { meetingId: 'routed-1' }]]);
  });

  test('an "already in progress" failure leaves the row alone', async () => {
    startResult = async () => { throw new Error('Recording already in progress'); };
    await mount();
    await act(async () => { await hook.handleRecordingStart().catch(() => {}); });
    expect(releaseCalls()).toEqual([]);
  });
});

describe('auto-start', () => {
  test('a continue auto-start skips routing', async () => {
    sessionStorage.setItem('autoStartRecording', 'true');
    sessionStorage.setItem('autoStartContinueId', 'earlier-1');
    await mount();
    expect(routeCalls()).toEqual([]);
    expect(startRecordingWithDevices).toHaveBeenCalledWith('mic', 'sys', null, null);
  });

  test('an auto-start with a meeting id routes to that page and passes the routed id', async () => {
    sessionStorage.setItem('autoStartRecording', 'true');
    sessionStorage.setItem('autoStartMeetingId', 'm-cal');
    await mount();
    expect(routeCalls()[0][1]).toEqual({ source: { page: 'm-cal' } });
    expect(startRecordingWithDevices).toHaveBeenCalledWith('mic', 'sys', 'Untitled · Mon 14:02', 'routed-1');
  });

  test('a failed auto-start releases the routed row', async () => {
    sessionStorage.setItem('autoStartRecording', 'true');
    startResult = async () => { throw new Error('device busy'); };
    const logged = console.error; console.error = () => {};
    const alerted = globalThis.alert; globalThis.alert = () => {};
    await mount();
    console.error = logged; globalThis.alert = alerted;
    expect(releaseCalls()).toEqual([['api_release_routed_start', { meetingId: 'routed-1' }]]);
  });
});
