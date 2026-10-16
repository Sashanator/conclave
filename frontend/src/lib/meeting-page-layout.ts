export type MeetingPageLayout = 'three-columns' | 'subtab';

const LAYOUT_KEY = 'conclave.meetingPageLayout';

export function parseMeetingPageLayout(v: unknown): MeetingPageLayout {
  return v === 'subtab' ? 'subtab' : 'three-columns';
}

export function readMeetingPageLayout(): MeetingPageLayout {
  try {
    return parseMeetingPageLayout(localStorage.getItem(LAYOUT_KEY));
  } catch {
    return 'three-columns';
  }
}

export function writeMeetingPageLayout(v: MeetingPageLayout): void {
  try { localStorage.setItem(LAYOUT_KEY, v); } catch { /* optional */ }
}

export function readStoredRatio(key: string, fallback: number, min: number, max: number): number {
  try {
    const raw = localStorage.getItem(key);
    const n = raw == null ? NaN : Number(raw);
    return Number.isFinite(n) && n >= min && n <= max ? n : fallback;
  } catch {
    return fallback;
  }
}

export function writeStoredRatio(key: string, value: number): void {
  try { localStorage.setItem(key, String(value)); } catch { /* layout persistence is optional */ }
}

export function readFlag(key: string): boolean {
  try { return localStorage.getItem(key) === '1'; } catch { return false; }
}

export function writeFlag(key: string, value: boolean): void {
  try { localStorage.setItem(key, value ? '1' : '0'); } catch { /* optional */ }
}
