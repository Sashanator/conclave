import { afterAll, beforeEach, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  sonner: { ...await import('sonner') },
  select: { ...await import('../../src/components/ui/select') },
  recording: { ...await import('../../src/contexts/RecordingStateContext') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/components/ui/select', () => originals.select);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.recording);
});

const recording = { isRecording: true, micFollowing: { device: null } };
const errors: string[] = [];
let release!: () => void;
const invoke = mock(async (command: string) => {
  if (command === 'get_audio_devices') return [{ name: 'A', device_type: 'Input' }];
  if (command === 'switch_recording_mic') { await new Promise<void>((res) => { release = res; }); throw new Error('Not recording'); }
  return null;
});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('sonner', () => ({ toast: { error: (m: string) => { errors.push(m); } } }));
// The real Select is a Radix portal; a bare element exposes the props that matter.
const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
mock.module('../../src/components/ui/select', () => ({
  Select: (p: { disabled?: boolean; onValueChange: (v: string) => void; children?: ReactNode }) => <div data-disabled={!!p.disabled} data-pick={p.onValueChange as never}>{p.children}</div>,
  SelectTrigger: passThrough, SelectContent: passThrough, SelectItem: passThrough, SelectValue: passThrough,
}));
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals.recording, useRecordingState: () => recording }));
const { MicPicker } = await import('../../src/components/MeetingPage/MicPicker');

beforeEach(() => { errors.length = 0; recording.isRecording = true; });

const mount = async () => {
  let r!: ReturnType<typeof create>;
  await act(async () => { r = create(<MicPicker value="A" />); });
  return r;
};
const select = (r: ReturnType<typeof create>) => r.root.find((n) => n.props['data-pick'] !== undefined);

test('the picker is enabled while recording and disabled once it has ended', async () => {
  const r = await mount();
  expect(select(r).props['data-disabled']).toBe(false);
  recording.isRecording = false;
  await act(async () => { r.update(<MicPicker value="A" />); });
  expect(select(r).props['data-disabled']).toBe(true);
});

test('a switch that fails after the recording stopped shows no toast', async () => {
  const r = await mount();
  let p!: Promise<void>;
  await act(async () => { p = select(r).props['data-pick']('B'); });
  recording.isRecording = false;
  await act(async () => { r.update(<MicPicker value="A" />); release(); await p; });
  expect(errors).toEqual([]);
});

test('a switch that fails while recording is still toasted', async () => {
  const r = await mount();
  let p!: Promise<void>;
  await act(async () => { p = select(r).props['data-pick']('B'); });
  await act(async () => { release(); await p; });
  expect(errors).toEqual(['Error: Not recording']);
});
