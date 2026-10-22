import { invoke } from '@tauri-apps/api/core';

/** Where a meeting's title came from. `calendar` keeps the event's uid and start. */
export interface TitleOrigin {
  source: 'default' | 'calendar';
  uid?: string;
  start?: string;
}

export type StartTitle = TitleOrigin & { title: string };

interface CalendarTitle {
  title: string;
  uid: string;
  start: string;
}

/** The calendar event a recording starting now belongs to names it; otherwise the generated title stays. */
export async function pickStartTitle(generated: string): Promise<StartTitle> {
  try {
    const event = await invoke<CalendarTitle | null>('calendar_title_now');
    if (event?.title.trim()) return { title: event.title, source: 'calendar', uid: event.uid, start: event.start };
  } catch {
    // The calendar is optional: a failure keeps the generated title.
  }
  return { title: generated, source: 'default' };
}

/** What a reload during a recording restores: the title's source and its calendar link, kept in sessionStorage. */
export interface StoredTitle {
  source: 'default' | 'calendar';
  calendar: { uid: string; start: string } | null;
}

/** Parses the stored title origin. Null when nothing is stored or the value is broken. */
export function parseStoredTitle(raw: string | null): StoredTitle | null {
  if (!raw) return null;
  try {
    const value = JSON.parse(raw);
    const calendar = value?.calendar;
    if (value?.source === 'calendar' && typeof calendar?.uid === 'string' && typeof calendar?.start === 'string') {
      return { source: 'calendar', calendar: { uid: calendar.uid, start: calendar.start } };
    }
    return { source: 'default', calendar: null };
  } catch {
    return null;
  }
}
