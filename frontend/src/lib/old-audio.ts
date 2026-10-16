export interface OldAudio { meeting_id: string; title: string; created_at: string; bytes: number; files: string[] }
export interface DeleteResult { meetings: number; bytes: number; failed: string[] }

export const REMINDER_DAY_KEY = 'conclave.oldAudioReminderDay';

export const OLD_AUDIO_OPTIONS = [
  { value: 'never', label: 'Never' },
  { value: '7', label: '7 days' },
  { value: '30', label: '30 days' },
  { value: '90', label: '90 days' },
] as const;

export function daysToSelect(days: number | null | undefined): string {
  if (days === null) return 'never';
  if (days === undefined) return '30';
  return String(days);
}

export function selectToDays(v: string): number | null {
  return v === 'never' ? null : Number(v);
}

export function formatMegabytes(bytes: number): string {
  const mb = Math.round(bytes / 1_000_000);
  return mb >= 1000 ? `${(mb / 1000).toFixed(1)} GB` : `${mb} MB`;
}

/** "1 recording" or "3 recordings" */
export function recordingCount(n: number): string {
  return `${n} ${n === 1 ? 'recording' : 'recordings'}`;
}

export function toggleId(selected: string[], id: string): string[] {
  return selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id];
}

export function shouldRemind(lastShownDay: string | null, today: string, count: number): boolean {
  return count > 0 && lastShownDay !== today;
}

export function todayKey(d = new Date()): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}
