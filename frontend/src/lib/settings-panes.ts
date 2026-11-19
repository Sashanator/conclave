export type SettingsPane = 'recording' | 'transcription' | 'speakers' | 'models' | 'summaries' | 'copilot' | 'templates' | 'names' | 'app';

export const SETTINGS_GROUPS: { label: 'Capture' | 'AI' | 'You' | 'App'; panes: { id: SettingsPane; label: string }[] }[] = [
  { label: 'Capture', panes: [{ id: 'recording', label: 'Recording & devices' }, { id: 'transcription', label: 'Transcription & vocabulary' }, { id: 'speakers', label: 'Speakers' }] },
  { label: 'AI', panes: [{ id: 'models', label: 'Models & keys' }, { id: 'summaries', label: 'Summaries' }, { id: 'copilot', label: 'Copilot' }, { id: 'templates', label: 'Templates' }] },
  { label: 'You', panes: [{ id: 'names', label: 'Names & calendar' }] },
  { label: 'App', panes: [{ id: 'app', label: 'Appearance & app' }] },
];

// Old tab ids (and every pane id) -> pane, keyed lowercase.
const TAB_ALIASES: Record<string, SettingsPane> = {
  recording: 'recording', transcription: 'transcription', speakers: 'speakers', models: 'models', summaries: 'summaries',
  copilot: 'copilot', templates: 'templates', names: 'names', app: 'app',
  general: 'app', transcriptionmodels: 'transcription', beta: 'transcription', summarymodels: 'summaries', summary: 'summaries',
  knowledge: 'names', costs: 'models',
};

export const isKnownTab = (tab: string): boolean => tab.toLowerCase() in TAB_ALIASES;

export const paneForTab = (tab: string | null): SettingsPane => TAB_ALIASES[(tab ?? '').toLowerCase()] ?? 'recording';

export const settingsHref = (pane: SettingsPane): string => `/settings?tab=${pane}`;

export const paneLabel = (p: SettingsPane): string => SETTINGS_GROUPS.flatMap((g) => g.panes).find((x) => x.id === p)!.label;

export const settingId = (label: string): string => `setting-${label.toLowerCase().replace(/[^a-z0-9]+/g, '-')}`;

const idx = (pane: SettingsPane, labels: string[]) => labels.map((label) => ({ pane, label }));

export const SETTINGS_INDEX: { pane: SettingsPane; label: string; helper?: string }[] = [
  ...idx('recording', ['Save audio recordings', 'Save location', 'Recording start notification', 'Windows notifications when recording starts/stops', 'Meeting detection', 'Remind me about audio older than', 'Default audio devices']),
  ...idx('transcription', ['Transcription model', 'Import audio & retranscribe', 'Vocabulary']),
  ...idx('speakers', ['Identify speakers', 'Known voices']),
  ...idx('models', ['Claude', 'OpenAI', 'Groq', 'OpenRouter', 'Custom server', 'Summary', 'Copilot', 'Tasks & digests', 'Template assistant', 'Dictate cleanup', 'Practice', 'Model prices']),
  ...idx('summaries', ['Auto summary', 'Summary language']),
  ...idx('copilot', ['Answer language']),
  ...idx('templates', ['Templates', 'Include the transcript by default']),
  ...idx('names', ['My names', 'Extract my tasks after each summary', 'Weekly digest', 'Alert me when someone says', 'My calls marker', 'Calendar']),
  ...idx('app', ['Theme', 'Data storage locations', 'Updates', 'Check for updates on start', 'GitHub token', 'Backups']),
];

export function searchSettings(q: string): { pane: SettingsPane; label: string }[] {
  const needle = q.trim().toLowerCase();
  if (!needle) return [];
  return SETTINGS_INDEX.filter((s) => `${s.label} ${s.helper ?? ''}`.toLowerCase().includes(needle)).map(({ pane, label }) => ({ pane, label }));
}
