import { afterAll, afterEach, beforeEach, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  sonner: { ...await import('sonner') },
  select: { ...await import('../../src/components/ui/select') },
  switchUi: { ...await import('../../src/components/ui/switch') },
  config: { ...await import('../../src/contexts/ConfigContext') },
  rec: { ...await import('../../src/contexts/RecordingStateContext') },
  dev: { ...await import('../../src/components/DeviceSelection') },
  old: { ...await import('../../src/components/OldAudio/OldAudioReview') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/components/ui/select', () => originals.select);
  mock.module('../../src/components/ui/switch', () => originals.switchUi);
  mock.module('../../src/contexts/ConfigContext', () => originals.config);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.rec);
  mock.module('../../src/components/DeviceSelection', () => originals.dev);
  mock.module('../../src/components/OldAudio/OldAudioReview', () => originals.old);
});

const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
const prefs = { save_folder: 'C:/rec', auto_save: true, file_format: 'mp4', preferred_mic_device: null, preferred_system_device: null, meeting_detection: 'ask', old_audio_days: 30 };
const invoke = mock(async (cmd: string): Promise<unknown> => {
  if (cmd === 'get_recording_preferences') return prefs;
  return null;
});
const toast = { success: mock(() => {}), error: mock((_a: string, _b?: unknown) => {}), info: mock(() => {}), warning: mock(() => {}) };
const notif = { notification_preferences: { show_recording_started: true, show_recording_stopped: true, other: 'keep' }, extra: 1 };
let notificationSettings: any = notif;
let updateFails = false;
const updateNotificationSettings = mock(async (_s: unknown) => { if (updateFails) throw new Error('no'); });
const toggleBetaFeature = mock((_k: string, _on: boolean) => {});
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));
mock.module('sonner', () => ({ toast }));
mock.module('../../src/contexts/ConfigContext', () => ({
  ...originals.config,
  useConfig: () => ({ notificationSettings, loadPreferences: async () => {}, updateNotificationSettings, betaFeatures: { importAndRetranscribe: false }, toggleBetaFeature, setSelectedDevices() {} }),
}));
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals.rec, useRecordingState: () => ({ isRecording: false }) }));
mock.module('../../src/components/DeviceSelection', () => ({ ...originals.dev, DeviceSelection: () => <div>stub:devices</div> }));
mock.module('../../src/components/OldAudio/OldAudioReview', () => ({ OldAudioReview: () => null }));
// A plain button stands in for the Radix switch, so a test can press it
mock.module('../../src/components/ui/switch', () => ({
  Switch: ({ checked, onCheckedChange, ...rest }: { checked?: boolean; onCheckedChange?: (on: boolean) => void; [key: string]: unknown }) => (
    <button type="button" role="switch" aria-checked={checked} onClick={() => onCheckedChange?.(!checked)} {...rest} />
  ),
}));
mock.module('../../src/components/ui/select', () => ({
  Select: passThrough, SelectContent: passThrough, SelectItem: () => null, SelectTrigger: () => null, SelectValue: () => null,
}));

const { NotificationsRow } = await import('../../src/components/Settings/NotificationsRow');
const { TranscriptionExtras } = await import('../../src/components/Settings/TranscriptionExtras');
const { RecordingSettings } = await import('../../src/components/RecordingSettings');

let r: ReactTestRenderer;
const flush = () => act(async () => { await new Promise((x) => setTimeout(x, 0)); });
const mount = async (el: JSX.Element) => { await act(async () => { r = create(el); }); await flush(); await flush(); };
const text = (n: ReactTestInstance): string => n.children.map((c) => (typeof c === 'string' ? c : text(c))).join('');
const sw = (name: string) => r.root.findAll((n) => n.type === 'button' && n.props.role === 'switch' && n.props['aria-label'] === name)[0];

afterEach(() => { act(() => r?.unmount()); });

beforeEach(() => {
  invoke.mockClear(); toast.success.mockClear(); toast.error.mockClear(); updateNotificationSettings.mockClear(); toggleBetaFeature.mockClear();
  notificationSettings = notif; updateFails = false;
});

test('notifications switch is named, turns both flags off and keeps the rest, shows Saved', async () => {
  await mount(<NotificationsRow />);
  const s = sw('Windows notifications when recording starts/stops');
  expect(s.props['aria-checked']).toBe(true);
  await act(async () => { await s.props.onClick(); });
  await flush();
  expect(updateNotificationSettings).toHaveBeenCalledWith({ ...notif, notification_preferences: { ...notif.notification_preferences, show_recording_started: false, show_recording_stopped: false } });
  expect(text(r.root)).toContain('Saved ✓');
});

test('a rejected notification update toasts and the switch is checked again', async () => {
  updateFails = true;
  await mount(<NotificationsRow />);
  await act(async () => { await sw('Windows notifications when recording starts/stops').props.onClick(); });
  await flush();
  expect(toast.error).toHaveBeenCalled();
  expect(sw('Windows notifications when recording starts/stops').props['aria-checked']).toBe(true);
});

test('Import row mirrors the beta flag and toggles it', async () => {
  await mount(<TranscriptionExtras />);
  const s = sw('Import audio & retranscribe');
  expect(s.props['aria-checked']).toBe(false);
  await act(async () => { s.props.onClick(); });
  expect(toggleBetaFeature).toHaveBeenCalledWith('importAndRetranscribe', true);
  expect(text(r.root)).toContain('Edit on Dictate');
});

test('meeting detection change saves, shows Saved and no success toast', async () => {
  await mount(<RecordingSettings />);
  const select = r.root.findAll((n) => n.props.onValueChange !== undefined && n.props.value === 'ask')[0];
  await act(async () => { await select.props.onValueChange('auto'); });
  await flush();
  expect(invoke).toHaveBeenCalledWith('set_recording_preferences', { preferences: { ...prefs, meeting_detection: 'auto' } });
  expect(text(r.root)).toContain('Saved ✓');
  expect(toast.success).not.toHaveBeenCalled();
  expect(text(r.root)).toContain('Windows notifications when recording starts/stops');
});
