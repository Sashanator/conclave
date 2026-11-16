// Dev-only: lets `pnpm run dev:preview` render the UI in a plain browser (design work, a11y checks) by faking the
// Tauri backend. Never active in a Tauri window (it has __TAURI_INTERNALS__) or a production build (NODE_ENV check
// folds this whole file away). Commands not listed here resolve null and warn once; add data here as screens need it.
import { mockIPC } from '@tauri-apps/api/mocks';
import type { MeetingRow } from './meetings';
import { BUILTIN_VIEW_DEFAULTS } from './meeting-views';

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

// Week-relative times, so the table shows this week's rows whatever day the preview runs.
const monday = (h: number, m = 0) => {
  const d = new Date();
  d.setDate(d.getDate() - ((d.getDay() + 6) % 7));
  d.setHours(h, m, 0, 0);
  return d.toISOString();
};

// Two days back from now: reads "not recorded" (over 24 h past) whatever day the preview runs.
const daysAgo = (n: number, h: number, m = 0) => {
  const d = new Date();
  d.setDate(d.getDate() - n);
  d.setHours(h, m, 0, 0);
  return d.toISOString();
};

const meetingRows = (): MeetingRow[] => {
  const base: MeetingRow = {
    id: '', title: '', titleSource: 'default', templateId: null, emoji: null, reviewed: false, gist: null,
    startedAt: null, endedAt: null, createdAt: monday(8), calendarUid: null, calendarStart: null, calendarEnd: null,
    calendarCancelledAt: null, hasTranscript: false, hasSummary: false, prepPoints: 0,
  };
  return [
    { ...base, id: 'preview-4', title: 'Release planning', reviewed: true, gist: 'Release is on track; the audio switch is the open risk.', startedAt: monday(10), endedAt: monday(10, 45), hasTranscript: true, hasSummary: true, createdAt: monday(9, 55) },
    { ...base, id: 'preview-5', title: 'Candidate interview', templateId: 'interview', emoji: '🤝', startedAt: monday(9), endedAt: monday(9, 50), hasTranscript: true, hasSummary: true, createdAt: monday(9) },
    { ...base, id: 'preview-3', title: 'Untitled · Mon 14:02', startedAt: monday(14, 2), endedAt: monday(14, 20), hasTranscript: true, createdAt: monday(14, 2) },
    { ...base, id: 'preview-7', title: 'Design review', titleSource: 'calendar', templateId: 'one_to_one', emoji: '👥', calendarUid: 'preview-uid-7', calendarStart: daysAgo(2, 11), calendarEnd: daysAgo(2, 11, 30), prepPoints: 2 },
    { ...base, id: 'preview-8', title: 'Budget check', titleSource: 'calendar', calendarUid: 'preview-uid-8', calendarStart: monday(13), calendarEnd: monday(13, 30), calendarCancelledAt: monday(12), prepPoints: 1 },
    { ...base, id: 'preview-9', title: 'Vendor call', titleSource: 'calendar', calendarUid: 'preview-uid-9', calendarStart: monday(15), calendarEnd: monday(15, 45), prepPoints: 3 },
    { ...base, id: 'preview-1', title: 'Weekly product sync', reviewed: true, startedAt: '2026-11-10T09:00:00Z', endedAt: '2026-11-10T09:45:00Z', createdAt: '2026-11-10T09:00:00Z', hasTranscript: true, hasSummary: true },
    { ...base, id: 'preview-2', title: 'Interview: backend engineer', reviewed: true, startedAt: '2026-11-11T14:00:00Z', endedAt: '2026-11-11T14:50:00Z', createdAt: '2026-11-11T14:00:00Z', hasTranscript: true, hasSummary: true },
  ];
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
  api_list_templates: () => ({ templates: [{ id: 'standard_meeting', name: 'Standard meeting', emoji: '📝' }, { id: 'one_to_one', name: '1:1', emoji: '👥' }, { id: 'interview', name: 'Interview', emoji: '🤝' }], errors: [] }),
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
  api_list_meeting_rows: () => meetingRows(),
  api_list_meeting_views: () => [
    { id: 'all', name: 'All meetings', position: 0, builtin: true, configJson: JSON.stringify(BUILTIN_VIEW_DEFAULTS.all) },
    { id: 'week', name: 'This week', position: 1, builtin: true, configJson: JSON.stringify(BUILTIN_VIEW_DEFAULTS.week) },
    { id: 'unreviewed', name: 'Not reviewed', position: 2, builtin: true, configJson: JSON.stringify(BUILTIN_VIEW_DEFAULTS.unreviewed) },
  ],
  api_create_meeting: () => 'preview-new',
  api_save_meeting_view: () => null,
  api_delete_meeting_view: () => null,
  api_set_meeting_reviewed: () => null,
  api_set_meeting_template: () => null,
  api_delete_meeting: () => ({ folder_warning: null }),
  api_search_transcripts: ({ query }) => (/plan/i.test(query)
    ? [{ meetingId: 'preview-4', title: 'Release planning', source: 'transcript', snippet: 'Let us start with the release plan for next week.', transcriptId: 't-0', audioStart: 0, totalInMeeting: 1 }]
    : []),
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
