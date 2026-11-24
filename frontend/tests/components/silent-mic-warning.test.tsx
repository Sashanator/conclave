import { afterAll, beforeEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestInstance } from 'react-test-renderer';

const originals = {
  event: { ...await import('@tauri-apps/api/event') },
  recording: { ...await import('../../src/contexts/RecordingStateContext') },
  micPicker: { ...await import('../../src/components/MeetingPage/MicPicker') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.recording);
  mock.module('../../src/components/MeetingPage/MicPicker', () => originals.micPicker);
});

const handlers = new Map<string, (e: { payload: unknown }) => void>();
mock.module('@tauri-apps/api/event', () => ({ listen: async (n: string, h: (e: { payload: unknown }) => void) => { handlers.set(n, h); return () => { handlers.delete(n); }; } }));
const state = { isPaused: false };
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals.recording, useRecordingState: () => state }));
let liveMic = 'A';
mock.module('../../src/components/MeetingPage/MicPicker', () => ({ MicPicker: () => null, useLiveMic: () => [liveMic, () => {}] }));
const { SilentMicWarning } = await import('../../src/components/MeetingPage/SilentMicWarning');

// A fake clock: Date.now and the 1 s poll are driven by hand.
const realNow = Date.now;
const realSetInterval = globalThis.setInterval;
const realClearInterval = globalThis.clearInterval;
let now = 0;
let tickFn: (() => void) | null = null;
afterAll(() => { Date.now = realNow; globalThis.setInterval = realSetInterval; globalThis.clearInterval = realClearInterval; });
Date.now = () => now;
globalThis.setInterval = ((fn: () => void) => { tickFn = fn; return 1; }) as never;
globalThis.clearInterval = (() => { tickFn = null; }) as never;

const text = (n: ReactTestInstance): string => n.children.map((c) => (typeof c === 'string' ? c : text(c))).join('');
let r!: ReturnType<typeof create>;
const mount = async () => { await act(async () => { r = create(<SilentMicWarning />); }); };
const advance = async (ms: number) => { for (let i = 0; i < ms / 1000; i++) { now += 1000; await act(async () => { tickFn!(); }); } };
const shown = () => r.toJSON() !== null;
const sound = (peak: number) => act(async () => { handlers.get('audio-levels')!({ payload: { levels: [{ device_type: 'input', peak_level: peak }] } }); });

beforeEach(async () => { now = 1_000_000; state.isPaused = false; liveMic = 'A'; await mount(); });

test('appears after 10 s of digital silence, not before', async () => {
  await advance(9000);
  expect(shown()).toBe(false);
  await advance(1000);
  expect(text(r.root)).toContain('No sound from A');
});

test('sound resets the timer', async () => {
  await advance(9000);
  await sound(0.01);
  await advance(9000);
  expect(shown()).toBe(false);
  await advance(1000);
  expect(shown()).toBe(true);
});

test('pausing resets the timer and shows nothing', async () => {
  await advance(8000);
  state.isPaused = true;
  await act(async () => { r.update(<SilentMicWarning />); });
  await advance(20000);
  expect(shown()).toBe(false);
  state.isPaused = false;
  await act(async () => { r.update(<SilentMicWarning />); });
  await advance(9000);
  expect(shown()).toBe(false);
  await advance(1000);
  expect(shown()).toBe(true);
});

test('dismiss hides the warning for that mic only', async () => {
  await advance(10000);
  await act(async () => { r.root.findByProps({ 'aria-label': 'Dismiss' }).props.onClick(); });
  expect(shown()).toBe(false);
  liveMic = 'B';
  await act(async () => { r.update(<SilentMicWarning />); });
  await advance(10000);
  expect(text(r.root)).toContain('No sound from B');
});

test('a mic change restarts the timer', async () => {
  await advance(9000);
  liveMic = 'B';
  await act(async () => { r.update(<SilentMicWarning />); });
  await advance(9000);
  expect(shown()).toBe(false);
  await advance(1000);
  expect(text(r.root)).toContain('No sound from B');
});
