import { afterAll, afterEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  sonner: { ...await import('sonner') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('sonner', () => originals.sonner);
});

type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();
const listen = mock(async (event: string, handler: Handler) => {
  handlers.set(event, handler);
  return () => { if (handlers.get(event) === handler) handlers.delete(event); };
});
let backend: unknown = { is_recording: false };
const invoke = mock(async (_command: string): Promise<unknown> => backend);
const toast = { success: mock(() => {}), error: mock(() => {}), info: mock(() => {}), warning: mock(() => {}) };
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('@tauri-apps/api/event', () => ({ listen }));
mock.module('sonner', () => ({ toast }));
const { RecordingStateProvider, useRecordingState } = await import('../../src/contexts/RecordingStateContext');

let live: string | null | undefined;
function Probe() { live = useRecordingState().liveMeetingId; return null; }

let renderer: ReactTestRenderer | undefined;
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined; handlers.clear(); live = undefined; backend = { is_recording: false };
});
async function mount() {
  await act(async () => { renderer = create(<RecordingStateProvider><Probe /></RecordingStateProvider>); });
  await act(async () => { await new Promise((r) => setTimeout(r, 0)); });
}

describe('liveMeetingId', () => {
  test('a fresh provider reads the live id from the backend without any event', async () => {
    backend = { is_recording: true, is_paused: false, is_active: true, meeting_id: 'm1' };
    await mount();
    expect(live).toBe('m1');
  });

  test('idle gives null', async () => {
    await mount();
    expect(live).toBeNull();
  });

  test('re-reads on recording-target and clears when recording stops', async () => {
    await mount();
    backend = { is_recording: true, is_paused: false, is_active: true, meeting_id: 'm2' };
    await act(async () => { handlers.get('recording-target')!({ payload: { meetingId: 'm2' } }); await new Promise((r) => setTimeout(r, 0)); });
    expect(live).toBe('m2');
    backend = { is_recording: false };
    await act(async () => { handlers.get('recording-stopped')!({ payload: { message: 'x' } }); await new Promise((r) => setTimeout(r, 0)); });
    expect(live).toBeNull();
  });

  test('a poll that began before recording-stopped cannot bring the id back', async () => {
    await mount();
    let release!: (v: unknown) => void;
    invoke.mockImplementationOnce(() => new Promise((r) => { release = r; }));
    await act(async () => { handlers.get('recording-target')!({ payload: {} }); });
    await act(async () => { handlers.get('recording-stopped')!({ payload: { message: 'x' } }); });
    await act(async () => { release({ is_recording: true, is_paused: false, is_active: true, meeting_id: 'stale' }); await new Promise((r) => setTimeout(r, 0)); });
    expect(live).toBeNull();
  });

  test('a refused target claim shows the new-meeting warning', async () => {
    await mount();
    await act(async () => { handlers.get('recording-target-failed')!({ payload: { reason: 'Its meeting was deleted.' } }); });
    expect(toast.warning).toHaveBeenCalledWith('Recording as a new meeting', { description: 'Its meeting was deleted.' });
  });
});
