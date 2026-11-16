// Dev-only: lets `pnpm run dev:preview` render the UI in a plain browser (design work, a11y checks) by faking the
// Tauri backend. Never active in a Tauri window (it has __TAURI_INTERNALS__) or a production build (NODE_ENV check
// folds this whole file away). Commands not listed here resolve null and warn once; add data here as screens need it.
import { mockIPC } from '@tauri-apps/api/mocks';

const meetings = [
  { id: 'preview-1', title: 'Weekly product sync', created_at: '2026-11-10T09:00:00Z', updated_at: '2026-11-10T09:45:00Z' },
  { id: 'preview-2', title: 'Interview: backend engineer', created_at: '2026-11-11T14:00:00Z', updated_at: '2026-11-11T14:50:00Z' },
  { id: 'preview-3', title: 'Standup', created_at: '2026-11-12T08:30:00Z', updated_at: '2026-11-12T08:45:00Z' },
];

const lines = [
  'Let us start with the release plan for next week.',
  'The installer build is green, the remaining risk is the audio device switch.',
  'I will test it on the Windows machine tomorrow and report back.',
  'Good. Then the redesign review moves to Thursday.',
];
const transcripts = lines.map((text, i) => ({
  id: `t-${i}`,
  text,
  timestamp: `09:0${i}:00`,
  sequence_id: i,
  audio_start_time: i * 12,
  audio_end_time: i * 12 + 10,
  duration: 10,
  speaker_idx: i % 2,
  mark: i === 2 ? 'action' : null,
  sentences: null,
}));

const summary = {
  markdown: '## Summary\n\nThe team agreed the release is on track.\n\n## Action items\n\n- Test the audio device switch on Windows (owner: Sam)\n- Move the redesign review to Thursday',
};

const handlers: Record<string, (args: any) => unknown> = {
  get_onboarding_status: () => ({ completed: true, current_step: 4, model_status: { parakeet: 'downloaded' } }),
  check_first_launch: () => false,
  get_recording_state: () => ({ is_recording: false, is_paused: false, is_active: false, recording_duration: null, active_duration: null }),
  get_recording_preferences: () => ({ preferred_mic_device: null, preferred_system_device: null, meeting_detection: 'off' }),
  api_get_model_config: () => ({ provider: 'claude', model: 'claude-sonnet-4-5', whisperModel: 'large-v3' }),
  api_get_transcript_config: () => ({ provider: 'localWhisper', model: 'large-v3' }),
  get_audio_devices: () => [{ name: 'Preview mic', device_type: 'Input' }, { name: 'Preview speakers', device_type: 'Output' }],
  get_ollama_models: () => [],
  api_list_templates: () => ({ templates: [{ id: 'standard_meeting', name: 'Standard meeting' }], errors: [] }),
  get_meeting_audio: () => ({ path: null }),
  get_meeting_notes: () => [],
  speakers_get_settings: () => ({ enabled: false, models_ready: false }),
  old_audio_list: () => [],
  updates_get_status: () => ({ tokenLast4: '3f9a', checkOnStart: true, lastCheck: { at: '2026-11-16T09:00:00Z', result: 'Up to date' }, currentVersion: '1.1.0' }),
  updates_check: () => ({ status: 'available', version: '1.2.0', notes: 'Faster transcript loading and safer upgrades.', message: null }),
  updates_set_token: () => null,
  updates_remove_token: () => null,
  updates_set_check_on_start: () => null,
  updates_install: () => null,
  backup_open_folder: () => null,
  backup_list: () => [
    { kind: 'manual', createdAt: '2026-11-16T09:30:00Z', fromVersion: null, version: '1.1.0', sizeBytes: 48_200_000, path: 'C:\\…' },
    { kind: 'pre-upgrade', createdAt: '2026-11-16T08:59:00Z', fromVersion: '1.0.4', version: '1.1.0', sizeBytes: 48_200_000, path: 'C:\\…' },
  ],
  backup_now: () => ({ kind: 'manual', createdAt: '2026-11-16T09:30:00Z', fromVersion: null, version: '1.1.0', sizeBytes: 48_200_000, path: 'C:\\…' }),
  feed_unread_count: () => 0,
  feed_list: () => [],
  api_get_meetings: () => meetings,
  api_get_meeting_metadata: ({ meetingId }) => meetings.find((m) => m.id === meetingId) ?? meetings[0],
  api_get_meeting_transcripts: () => ({ transcripts, total_count: transcripts.length, has_more: false }),
  api_get_summary: ({ meetingId }) => ({
    status: 'completed', meetingName: null, meeting_id: meetingId, start: null, end: null, data: summary, error: null,
  }),
};

const warned = new Set<string>();

export function installBrowserPreview() {
  if (
    process.env.NODE_ENV === 'production' ||
    process.env.NEXT_PUBLIC_BROWSER_PREVIEW !== '1' ||
    typeof window === 'undefined' ||
    (window as any).__TAURI_INTERNALS__
  ) return;
  (window as any).__TAURI_OS_PLUGIN_INTERNALS__ = { platform: 'windows', eol: '\r\n' };
  mockIPC((cmd, args) => {
    const h = handlers[cmd];
    if (h) return h(args);
    if (!cmd.startsWith('plugin:event|') && !warned.has(cmd)) {
      warned.add(cmd);
      console.warn(`[browser-preview] unmocked command "${cmd}" -> null`);
    }
    return null;
  }, { shouldMockEvents: true });
}
