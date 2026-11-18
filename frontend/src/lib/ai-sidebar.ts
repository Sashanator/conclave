export type ScreenKind = 'table' | 'live' | 'meeting';
export type AskScope = { label: string; meetingIds: string[] | null }; // null = all meetings

const DEFAULTS: Record<ScreenKind, boolean> = { live: true, table: false, meeting: false };
const key = (kind: ScreenKind) => `conclave.aiSidebar.open.${kind}`;

export function readSidebarOpen(kind: ScreenKind): boolean {
  try {
    const v = localStorage.getItem(key(kind));
    return v === null ? DEFAULTS[kind] : v === 'true';
  } catch {
    return DEFAULTS[kind];
  }
}

export function writeSidebarOpen(kind: ScreenKind, open: boolean): void {
  try {
    localStorage.setItem(key(kind), String(open));
  } catch {
    // storage unavailable: the state stays in memory only
  }
}
