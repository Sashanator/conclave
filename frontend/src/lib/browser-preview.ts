// Dev-only: lets `pnpm run dev:preview` render the UI in a plain browser (design work, a11y checks) by faking the
// Tauri backend. Never active in a Tauri window (it has __TAURI_INTERNALS__) or a production build (NODE_ENV check
// folds this whole file away). Commands not listed here resolve null and warn once; add data here as screens need it.
import { mockIPC } from '@tauri-apps/api/mocks';
import type { MeetingRow } from './meetings';
import { BLOCKING_STATES, type CategoryDefault, type DbSchema, type Destination, type DestStatus, type SendReport } from './notion';
import type { TaskRow } from './tasks';
import { BUILTIN_VIEW_DEFAULTS } from './meeting-views';

const meetings = [
  { id: 'preview-1', title: 'Weekly product sync', created_at: '2026-11-10T09:00:00Z', updated_at: '2026-11-10T09:45:00Z' },
  { id: 'preview-2', title: 'Interview: backend engineer', created_at: '2026-11-11T14:00:00Z', updated_at: '2026-11-11T14:50:00Z' },
  { id: 'preview-new', title: 'New meeting', created_at: '2026-11-17T08:00:00Z', updated_at: '2026-11-17T08:00:00Z' },
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
    calendarCancelledAt: null, hasTranscript: false, hasSummary: false, prepPoints: 0, hasPrep: false,
  };
  return [
    { ...base, id: 'preview-4', title: 'Release planning', reviewed: true, gist: 'Release is on track; the audio switch is the open risk.', startedAt: monday(10), endedAt: monday(10, 45), hasTranscript: true, hasSummary: true, createdAt: monday(9, 55) },
    { ...base, id: 'preview-5', title: 'Candidate interview', templateId: 'interview', emoji: '🤝', startedAt: monday(9), endedAt: monday(9, 50), hasTranscript: true, hasSummary: true, createdAt: monday(9) },
    { ...base, id: 'preview-3', title: 'Untitled · Mon 14:02', startedAt: monday(14, 2), endedAt: monday(14, 20), hasTranscript: true, createdAt: monday(14, 2) },
    { ...base, id: 'preview-7', title: 'Design review', titleSource: 'calendar', templateId: 'one_to_one', emoji: '👥', calendarUid: 'preview-uid-7', calendarStart: daysAgo(2, 11), calendarEnd: daysAgo(2, 11, 30), prepPoints: 2 },
    { ...base, id: 'preview-8', title: 'Budget check', titleSource: 'calendar', calendarUid: 'preview-uid-8', calendarStart: monday(13), calendarEnd: monday(13, 30), calendarCancelledAt: monday(12), prepPoints: 1 },
    { ...base, id: 'preview-9', title: 'Vendor call', titleSource: 'calendar', calendarUid: 'preview-uid-9', calendarStart: monday(15), calendarEnd: monday(15, 45), prepPoints: 3 },
    { ...base, id: 'preview-new', title: 'New meeting', createdAt: monday(8) },
    { ...base, id: 'preview-1', title: 'Weekly product sync', templateId: 'standard_meeting', emoji: '📝', reviewed: true, startedAt: '2026-11-10T09:00:00Z', endedAt: '2026-11-10T09:45:00Z', createdAt: '2026-11-10T09:00:00Z', hasTranscript: true, hasSummary: true },
    { ...base, id: 'preview-2', title: 'Interview: backend engineer', reviewed: true, startedAt: '2026-11-11T14:00:00Z', endedAt: '2026-11-11T14:50:00Z', createdAt: '2026-11-11T14:00:00Z', hasTranscript: true, hasSummary: true },
  ];
};


// ---- Redesign screens: tasks, prep, briefs, Ask, Copilot, settings, stats, Dictate, Practice, What's new ----
const row = (id: string, text: string, over: Partial<TaskRow> = {}): TaskRow => ({
  id, meetingId: 'preview-1', meetingTitle: 'Weekly product sync', meetingDate: '2026-11-10T09:00:00Z', text, owner: 'me', due: null, at: null, status: 'open', source: 'auto', createdAt: '2026-11-10T09:46:00Z',
  review: 'open', removedBy: null, destId: null, dueDate: null, notionUrl: null, sentAt: null, sendError: null, sendAmbiguous: false, templateId: 'standard_meeting', ...over,
});
const SENT = { review: 'sent' as const, notionUrl: 'https://www.notion.so/preview', sentAt: '2026-11-16T12:04:00Z' };
const tasks: TaskRow[] = [
  row('task-1', 'Test the audio device switch on the Windows machine', { destId: 'd-job', dueDate: '2026-11-26', at: 24 }),
  row('task-2', 'Send the updated price list to the vendor', { destId: 'd-job', at: 36 }),
  row('task-3', 'Book the dentist before the trip', { destId: 'd-personal', at: 48 }),
  row('task-4', 'Ask the vendor about the invoice', { owner: 'unassigned', status: 'dismissed', review: 'drop', removedBy: 'ai', at: 12 }),
  row('task-5', 'Share the release notes with the team', { ...SENT, destId: 'd-job', at: 60 }),
  row('task-6', 'Update the roadmap page', { ...SENT, destId: 'd-job', at: 70 }),
  row('task-7', 'Send the interview feedback to HR', { meetingId: 'preview-5', meetingTitle: 'Candidate interview', meetingDate: '2026-11-17T10:00:00Z', templateId: 'interview', review: 'keep', destId: 'd-job' }),
  row('task-8', 'Schedule the second interview round', { meetingId: 'preview-5', meetingTitle: 'Candidate interview', meetingDate: '2026-11-17T10:00:00Z', templateId: 'interview', review: 'keep', destId: 'd-job', sendAmbiguous: true }),
  row('task-9', 'Reply to the recruiter', { meetingId: 'preview-5', meetingTitle: 'Candidate interview', meetingDate: '2026-11-17T10:00:00Z', templateId: 'interview', review: 'keep', destId: 'd-job', sendError: 'Notion did not answer in time.' }),
  row('task-10', 'Call Max back about the contract', { meetingId: null, meetingTitle: 'Call with Max', meetingDate: '2026-11-09T19:30:00Z', templateId: null }),
];
const updateTask = (id: string, patch: Partial<TaskRow>) => {
  const t = tasks.find((x) => x.id === id);
  if (!t) throw new Error('task not found');
  Object.assign(t, patch);
  return t;
};
// ---- Notion fakes ----
const NOT_SHARED = "Personal tasks isn't shared with Conclave, or was deleted. In Notion: database ••• → Connections → add Conclave";
const mapping = { titleProp: 'Name', dueProp: 'Due', meetingProp: null, fixed: [{ prop: 'Status', kind: 'status' as const, value: 'To do' }] };
const mkDest = (id: string, name: string, colour: number, position: number, lastStatus: Destination['lastStatus'] = null, lastMessage: string | null = null): Destination =>
  ({ id, name, colour, position, databaseId: `${id}-database`, dataSourceId: null, mapping, lastStatus, lastMessage });
const destinations: Destination[] = [mkDest('d-job', 'Job tasks', 1, 0), mkDest('d-personal', 'Personal tasks', 2, 1, 'not_shared', NOT_SHARED), mkDest('d-german', 'German homework', 3, 2)];
let tokenLast4: string | null = '7Qx2';
const categoryDefaults: CategoryDefault[] = [{ templateKey: 'standard_meeting', destId: 'd-job' }, { templateKey: 'interview', destId: 'd-job' }, { templateKey: '', destId: null }];
const prop = (name: string, kind: string, options: string[] = []) => ({ id: name.toLowerCase(), name, kind, options });
const schema: DbSchema = {
  databaseId: 'preview-database', dataSourceId: null, title: 'Tasks',
  properties: [prop('Name', 'title'), prop('Due', 'date'), prop('Notes', 'rich_text'), prop('Status', 'status', ['To do', 'Doing', 'Done']), prop('Tags', 'multi_select', ['Work', 'Home']), prop('Urgent', 'checkbox'), prop('Owner', 'people'), prop('Area', 'select', ['Job', 'Life'])],
};
const sendRows = (ids: string[]): SendReport => {
  const r: SendReport = { sent: 0, failed: 0, ambiguous: 0 };
  for (const id of ids) {
    const t = tasks.find((x) => x.id === id);
    const d = destinations.find((x) => x.id === t?.destId);
    if (!t) continue;
    if (!d || !tokenLast4 || (d.lastStatus && BLOCKING_STATES.includes(d.lastStatus))) {
      t.sendError = d ? `${d.name} is not connected.` : 'Choose a Notion database first.';
      r.failed++;
    } else Object.assign(t, { review: 'sent', sentAt: new Date().toISOString(), notionUrl: 'https://www.notion.so/preview', sendError: null, sendAmbiguous: false }), r.sent++;
  }
  return r;
};

const prepItems = [
  { id: 'i-1', text: 'Confirm the budget owner', kind: 'ask', ticked: false, byUser: false },
  { id: 'i-2', text: 'Clarify the delivery date', kind: 'clarify', ticked: true, byUser: false },
];
const mkPrep = (over: Record<string, unknown> = {}) => ({
  id: 1, eventUid: 'preview-uid-9', eventStart: monday(15), eventTitle: 'Vendor call', templateId: null,
  material: 'Vendor sent the price list on Monday.', chat: [], items: prepItems, approvedAt: null,
  createdAt: '2026-11-16T09:00:00Z', updatedAt: '2026-11-16T09:00:00Z', ...over,
});

const stamp = (n: number) => { const d = new Date(); d.setDate(d.getDate() - n * 7 - ((d.getDay() + 6) % 7)); return d.toISOString().slice(0, 10); };
const practiceSession = { id: 'ps-1', setup_name: 'Salary talk', started_at: '2026-11-15T18:00:00Z', duration_secs: 540, report_status: 'ready', summary: 'Good pace; watch the fillers.', end_reason: 'user' };
const germanSession = { ...practiceSession, id: 'ps-2', setup_name: 'Gehaltsgespräch', started_at: '2026-11-16T18:00:00Z' };
const practiceReport = {
  version: 2,
  fluency: { fillers: { um: 14, like: 8 }, filler_total: 22, words: 1200, speech_secs: 540, wpm: 133.3, long_pauses: 4 },
  items: [
    { kind: 'Grammar', turn: 1, quote: 'If I will have more data…', better: 'If I had more data…', note: 'Past simple after "if".', echoed: true, pattern: null },
    { kind: 'Unnatural', turn: 3, quote: 'I made a research about it', better: 'I did some research on it', note: 'Native speakers "do" research.', echoed: false, pattern: null },
  ],
  dropped_items: 0, pronunciation: [], pronunciation_note: null,
};
const practiceTurns = [
  { session_id: 'ps-1', idx: 0, speaker: 'character', live_text: 'So, why do you think you deserve a raise?', verbatim_text: null, at_secs: 0, wav_offset: null, wav_len: null },
  { session_id: 'ps-1', idx: 1, speaker: 'user', live_text: 'If I will have more data', verbatim_text: 'Um, if I will have more data', at_secs: 12, wav_offset: null, wav_len: null },
];
const practiceDetail = (id: string) => ({
  summary: { ...practiceSession, id }, character: 'A hiring manager, polite but firm', turns: practiceTurns,
  report: id === 'ps-2'
    ? { ...practiceReport, pronunciation_note: 'Pronunciation hints are English-only, so this German session has none' }
    : practiceReport,
  report_error: null, audio_path: null, recurring: null,
});
const dictateSettings = { enabled: true, preferred_mic_device: null, main_hotkey: 'Ctrl+Alt+D', default_mode: 'cleaned-up', cleanup_provider: null, cleanup_model: null, builtin_hotkeys: {}, modes: [], dictionary: ['Conclave'] };
const knowledge = { provider: null, model: null, my_names: ['Sam'], extract_enabled: true, digest_enabled: true, calendar_marker: '📞' };

const extraHandlers: Record<string, (args: any) => unknown> = {
  get_version: () => '2.0.0',
  'plugin:app|version': () => '2.0.0',
  open_log_file: () => null,
  open_database_folder: () => null,
  get_database_directory: () => 'C:\\Users\\preview\\AppData\\Roaming\\Conclave',
  get_notification_settings: () => ({
    recording_notifications: true, time_based_reminders: true, meeting_reminders: true, respect_do_not_disturb: true, notification_sound: false,
    system_permission_granted: true, consent_given: true, manual_dnd_mode: false,
    notification_preferences: { show_recording_started: true, show_recording_stopped: true, show_recording_paused: false, show_recording_resumed: false, show_transcription_complete: true, show_meeting_reminders: true, show_system_errors: true, meeting_reminder_minutes: [5] },
  }),
  get_meeting_detection_status: () => null,
  get_transcription_status: () => ({ chunks_in_queue: 0, is_processing: false, last_activity_ms: 0 }),
  template_assist_get_settings: () => ({ provider: null, model: null, includeTranscript: true, hideEditHint: false }),
  costs_get_prices: () => [{ provider: 'claude', model: 'claude-sonnet-4-5', input: 3, output: 15, cache_read: 0.3, cache_write: 3.75, built_in: true }],
  meeting_cost: () => ({ total_usd: 0.042, tokens: 18400, by_purpose: [{ purpose: 'summary', tokens: 15200, cost_usd: 0.035 }, { purpose: 'extraction', tokens: 3200, cost_usd: 0.007 }] }),
  people_list: () => [],
  // Review queue and Notion: everything works on the in-memory lists above
  tasks_queue_list: () => tasks,
  tasks_set_review: ({ id, review }) => {
    if (['sent', 'sending'].includes(tasks.find((x) => x.id === id)?.review ?? '')) throw new Error('task not found or already sent');
    const patch = { open: { status: 'open', removedBy: null }, keep: { status: 'open', removedBy: null }, drop: { status: 'dismissed', removedBy: 'user' } }[review as 'open'];
    if (!patch) throw new Error('unknown review state');
    updateTask(id, { ...patch, review, owner: 'me' } as Partial<TaskRow>);
    return null;
  },
  tasks_set_dest: ({ id, destId }) => { if (destId && !destinations.some((d) => d.id === destId)) throw new Error('unknown destination'); updateTask(id, { destId: destId ?? null }); return null; },
  tasks_set_due_date: ({ id, dueDate }) => { updateTask(id, { dueDate: dueDate ?? null }); return null; },
  tasks_mark_sent: ({ id }) => { updateTask(id, { review: 'sent', sendAmbiguous: false, sentAt: new Date().toISOString() }); return null; },
  tasks_edit: ({ id, text }) => { updateTask(id, { text }); return null; },
  tasks_set_status: ({ id, status }) => { updateTask(id, { status }); return null; },
  notion_get_config: () => ({ tokenLast4, destinations, categoryDefaults }),
  notion_set_token: ({ token }) => { tokenLast4 = String(token).slice(-4); return null; },
  notion_remove_token: () => { tokenLast4 = null; return null; },
  notion_test: () => {
    const out: DestStatus[] = [{ destId: '', state: tokenLast4 ? 'connected' : 'token_missing', message: null }];
    for (const d of destinations) {
      d.lastStatus = d.id === 'd-personal' ? 'not_shared' : 'connected';
      d.lastMessage = d.id === 'd-personal' ? NOT_SHARED : null;
      out.push({ destId: d.id, state: d.lastStatus, message: d.lastMessage });
    }
    return out;
  },
  notion_read_columns: () => schema,
  notion_save_destination: ({ dest }) => {
    const saved: Destination = { ...dest, id: dest.id || `d-${destinations.length + 1}` };
    const i = destinations.findIndex((d) => d.id === saved.id);
    if (i >= 0) destinations[i] = saved; else destinations.push(saved);
    return saved;
  },
  notion_delete_destination: ({ id }) => {
    const i = destinations.findIndex((d) => d.id === id);
    if (i >= 0) destinations.splice(i, 1);
    tasks.forEach((t) => { if (t.destId === id) t.destId = null; });
    categoryDefaults.forEach((c) => { if (c.destId === id) c.destId = null; });
    return null;
  },
  notion_set_category_default: ({ templateKey, destId }) => {
    const c = categoryDefaults.find((x) => x.templateKey === templateKey);
    if (c) c.destId = destId ?? null; else categoryDefaults.push({ templateKey, destId: destId ?? null });
    return null;
  },
  notion_send: ({ taskIds }) => sendRows(taskIds ?? []),
  // Prep: a calendar meeting has one, other meetings none
  prep_for_meeting: ({ meetingId }) => (['preview-7', 'preview-9'].includes(meetingId) ? mkPrep() : null),
  prep_get: () => mkPrep(),
  prep_save: (a) => mkPrep({ eventTitle: a.eventTitle, material: a.material, templateId: a.templateId, items: a.items }),
  prep_tick: ({ itemId, ticked }) => mkPrep({ items: prepItems.map((i) => (i.id === itemId ? { ...i, ticked } : i)) }),
  prep_approve: () => mkPrep({ approvedAt: '2026-11-17T08:00:00Z' }),
  prep_material_limit: () => 20000,
  prep_send: ({ message }) => {
    const at = '2026-11-17T08:01:00Z';
    return { listUpdated: true, prep: mkPrep({ chat: [{ role: 'user', text: message, at }, { role: 'assistant', text: 'Added two questions about pricing and the delivery date to your list.', at }], items: [...prepItems, { id: 'i-3', text: 'Ask about volume discounts', kind: 'ask', ticked: false, byUser: false }] }) };
  },
  // Briefs and digests
  brief_for_meeting: ({ meetingId }) => (['preview-1', 'preview-9'].includes(meetingId) ? { isCall: true, briefId: 'brief-1' } : { isCall: false, briefId: null }),
  brief_regenerate: () => null,
  brief_meeting: () => 'preview-1',
  briefs_list: () => [
    { id: 'brief-1', kind: 'brief', title: 'Vendor call', createdAt: '2026-11-17T07:00:00Z' },
    { id: 'digest-1', kind: 'digest', title: 'Week of 9 November', createdAt: '2026-11-16T07:00:00Z' },
    { id: 'digest-2', kind: 'digest', title: 'Week of 2 November', createdAt: '2026-11-09T07:00:00Z' },
  ],
  brief_get: () => ({
    eventTitle: 'Vendor call', eventStart: monday(15), templateName: 'Standard meeting', previousMeetingId: 'preview-1', previousTitle: 'Weekly product sync', previousDate: '2026-11-10T09:00:00Z',
    requests: ['Send the updated price list'], decisions: ['Release goes out next week'], openTasks: tasks.filter((t) => t.status === 'open' && t.meetingId),
  }),
  digest_get: () => ({
    periodStart: '2026-11-09', periodEnd: '2026-11-15',
    markdown: '## Highlights\n\n- The release is on track, the audio device switch is the open risk [M1].\n- The redesign review moved to Thursday [M2].',
    meetingLinks: [{ id: 'preview-1', title: 'Weekly product sync' }, { id: 'preview-4', title: 'Release planning' }, { id: '', title: 'Deleted meeting' }],
  }),
  // Ask and Copilot
  ask_meetings: () => ({
    text: 'The release is on track; the only open risk is the audio device switch [E1].',
    citations: [{ id: 'E1', meetingId: 'preview-1', meetingTitle: 'Weekly product sync', date: '2026-11-10T09:00:00Z', transcriptId: 't-1', audioStart: 12, source: 'transcript' }],
  }),
  ask_cancel: () => null,
  copilot_get_settings: () => ({ provider: null, model: null, answer_language: null, alert_words: ['deadline'] }),
  copilot_save_settings: ({ settings }) => settings,
  copilot_ask: () => 'Short answer: the installer is green; the audio switch still needs a test on Windows (0:12).',
  copilot_cancel: () => null,
  // Knowledge and calendar
  knowledge_get_settings: () => knowledge,
  knowledge_save_settings: ({ input }) => {
    const marker = String(input?.calendar_marker ?? '');
    if ([...marker].length < 1 || [...marker].length > 16) throw 'The calls marker must be 1–16 characters';
    Object.assign(knowledge, input);
    return { ...knowledge };
  },
  calendar_status: () => ({ connected: true, host: 'calendar.example.com', last_ok_at: '2026-11-17T07:55:00Z', last_error: null, last_error_at: null, upcoming: 4 }),
  calendar_connect: () => ({ connected: true, host: 'calendar.example.com', last_ok_at: '2026-11-17T07:55:00Z', last_error: null, last_error_at: null, upcoming: 4 }),
  calendar_disconnect: () => ({ connected: false, host: null, last_ok_at: null, last_error: null, last_error_at: null, upcoming: 0 }),
  // Stats
  stats_cost: ({ count }) => Array.from({ length: count }, (_, i) => {
    const n = i;
    return { start: stamp(n), total_usd: n === 0 ? 0.31 : 0.2 + (n % 3) * 0.1, providers: [{ provider: 'claude', input_tokens: 120000, output_tokens: 18000, cost_usd: n === 0 ? 0.31 : 0.2 + (n % 3) * 0.1, unpriced_calls: 0 }] };
  }),
  stats_talk_time: () => ({
    current: [{ template_id: 'standard_meeting', template_name: 'Standard meeting', meetings: 4, self_seconds: 1500, total_seconds: 7200 }, { template_id: 'interview', template_name: 'Interview', meetings: 1, self_seconds: 600, total_seconds: 3000 }],
    previous: [{ template_id: 'standard_meeting', template_name: 'Standard meeting', meetings: 3, self_seconds: 1800, total_seconds: 5400 }],
  }),
  stats_talk_time_meetings: () => [{ meeting_id: 'preview-1', title: 'Weekly product sync', date: '2026-11-10', self_seconds: 540, total_seconds: 2700 }],
  // Dictate
  dictate_get_settings: () => dictateSettings,
  api_save_provider_key: () => null,
  template_assist_save_settings: () => null,
  set_recording_preferences: () => null,
  set_notification_settings: () => null,
  dictate_save_settings: ({ settings }) => settings,
  app_log: () => null,
  dictate_status: () => ({ enabled: true, model_downloaded: true, mic_ok: true, engine_is_whisper: true, reason: null }),
  // Practice
  practice_status: () => ({ has_claude_key: true, engine_is_whisper: true, voices_ready: true, blocked: null, running_session: null }),
  practice_list_setups: () => [{ id: 'setup-1', name: 'Salary talk', character: 'A hiring manager, polite but firm', situation: 'Negotiating a raise', voice: 'F1', language: 'en', seeded: false, has_plan: true, materials: [], session_count: 1 },
    { id: 'setup-2', name: 'Gehaltsgespräch', character: 'Eine Personalleiterin, freundlich aber bestimmt', situation: 'Gehaltsverhandlung', voice: 'F1', language: 'de', seeded: false, has_plan: false, materials: [], session_count: 1 }],
  practice_list_sessions: () => [practiceSession, germanSession],
  practice_get_session: ({ id }) => practiceDetail(id),
  practice_get_settings: () => ({ chat_model: 'claude-sonnet-5-5', plan_model: 'claude-opus-5-5', plan_effort: 'high' }),
  practice_save_settings: ({ settings }) => settings,
  practice_download_voices: () => null,
};

export const previewHandlers: Record<string, (args: any) => unknown> = {
  get_whisper_backend_status: () => ({ label: 'CUDA (preview)', cuda_runtime_missing: false }),
  set_language_preference: () => null,
  set_allowed_languages: () => null,
  set_recording_post_processing: () => null,
  api_get_api_key: ({ provider }) => (provider === 'claude' ? 'sk-preview-key' : ''),
  api_get_custom_openai_config: () => null,
  extraction_status: () => null,
  api_get_meeting_summary_language: () => ({ language: null, storage: 'metadata' }),
  whisper_get_models_directory: () => 'C:\\Users\\preview\\AppData\\Roaming\\Conclave\\models',
  get_default_recordings_folder_path: () => 'C:\\Users\\preview\\Music\\Conclave',
  get_onboarding_status: () =>
    location.search.includes('onboarding=1')
      ? { completed: false, current_step: 1, model_status: { parakeet: 'not_downloaded', summary: 'not_used' } }
      : { completed: true, current_step: 4, model_status: { parakeet: 'downloaded' } },
  get_meeting_banner: () => {
    const kind = new URLSearchParams(location.search).get('kind') ?? 'detected';
    return { kind, app: 'Zoom', certain: true, line: null, continue_title: kind === 'ended' ? null : 'Weekly sync with the platform team' };
  },
  meeting_banner_action: () => null,
  check_first_launch: () => false,
  get_recording_state: () => ({ is_recording: false, is_paused: false, is_active: false, recording_duration: null, active_duration: null }),
  get_recording_preferences: () => ({ preferred_mic_device: null, preferred_system_device: null, meeting_detection: 'off' }),
  api_get_model_config: () => ({ provider: 'claude', model: 'claude-sonnet-4-5', whisperModel: 'large-v3' }),
  api_get_transcript_config: () => ({ provider: 'localWhisper', model: 'large-v3' }),
  get_audio_backend_info: () => [],
  get_current_audio_backend: () => 'default',
  api_save_meeting_title: () => null,
  get_audio_devices: () => [{ name: 'Preview mic', device_type: 'Input' }, { name: 'Preview speakers', device_type: 'Output' }],
  get_ollama_models: () => [],
  take_backfill_error: () => null,
  'plugin:store|load': () => 1,
  'plugin:store|get': () => [null, false],
  'plugin:store|set': () => null,
  'plugin:store|save': () => null,
  whisper_init: () => null,
  whisper_get_available_models: () => [
    { name: 'large-v3-turbo', size_mb: 1620, status: 'Available' },
    { name: 'small', size_mb: 466, status: 'Missing' },
  ],
  api_get_auto_generate_setting: () => true,
  get_anthropic_models: () => [{ id: 'claude-sonnet-4-5', display_name: 'Claude Sonnet 4.5' }, { id: 'claude-haiku-4-5', display_name: 'Claude Haiku 4.5' }],
  api_get_template: (a) => ({
    name: ({ one_to_one: '1:1', interview: 'Interview' } as Record<string, string>)[a?.templateId] ?? 'Standard meeting',
    emoji: '📝', description: 'Decisions, actions and open questions.', use_when: 'Any regular meeting.', title_keywords: ['sync', 'weekly'], language: 'auto',
    sections: [{ title: 'Decisions', instruction: 'List each decision with its owner.', format: 'list' }, { title: 'Action items', instruction: 'Who does what, by when.', format: 'list' }],
  }),
  save_onboarding_status_cmd: () => null,
  dictate_pill_view: () => ({ state: 'listening', elapsed_ms: 4200, level: 0.4, mode: 'plain' }),
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
    { kind: 'manual', createdAt: '2026-11-16T09:30:00Z', fromVersion: null, version: '1.1.0', sizeBytes: 48_200_000, path: 'C:\\Conclave\\backups\\manual' },
    { kind: 'pre-upgrade', createdAt: '2026-11-16T08:59:00Z', fromVersion: '1.0.4', version: '1.1.0', sizeBytes: 48_200_000, path: 'C:\\Conclave\\backups\\pre-upgrade' },
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
  api_delete_recording: () => ({ folder_warning: null }),
  ...extraHandlers,
  api_search_transcripts: ({ query }) => (/plan/i.test(query)
    ? [{ meetingId: 'preview-4', title: 'Release planning', source: 'transcript', snippet: 'Let us start with the release plan for next week.', transcriptId: 't-0', audioStart: 0, totalInMeeting: 1 }]
    : []),
  api_route_recording_start: () => ({ meetingId: 'preview-1', title: 'Weekly sync' }),
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
    const h = previewHandlers[cmd];
    if (h) return h(args);
    if (!cmd.startsWith('plugin:event|') && !warned.has(cmd)) {
      warned.add(cmd);
      console.warn(`[browser-preview] unmocked command "${cmd}" -> null`);
    }
    return null;
  }, { shouldMockEvents: true });
}
