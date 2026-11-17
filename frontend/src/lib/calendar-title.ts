/** Where a meeting's title came from. `calendar` keeps the event's uid and start. */
export interface TitleOrigin {
  source: 'default' | 'calendar';
  uid?: string;
  start?: string;
}

export type StartTitle = TitleOrigin & { title: string };

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
