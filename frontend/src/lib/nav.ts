import type { LucideIcon } from 'lucide-react';
import { AudioLines, BarChart3, GraduationCap, Home, ListChecks, Newspaper } from 'lucide-react';
import { formatClock } from '@/lib/practice';

export type NavItem = { label: string; href: string; icon: LucideIcon; match: string[] };

export const NAV_MAIN: NavItem[] = [
  { label: 'Meetings', href: '/', icon: Home, match: ['/', '/meeting-details'] },
  { label: 'Tasks', href: '/tasks', icon: ListChecks, match: ['/tasks'] },
  { label: 'Digests', href: '/digests', icon: Newspaper, match: ['/digests'] },
  { label: 'Dictate', href: '/dictate', icon: AudioLines, match: ['/dictate'] },
  { label: 'Practice', href: '/practice', icon: GraduationCap, match: ['/practice'] },
  { label: 'Stats', href: '/stats', icon: BarChart3, match: ['/stats'] },
];

export function isNavActive(pathname: string, item: NavItem): boolean {
  return item.match.some((m) => (m === '/' ? pathname === '/' : pathname === m || pathname.startsWith(`${m}/`)));
}

export function recordLabel(isRecording: boolean, activeSecs: number | null): string {
  return isRecording ? `Recording ${formatClock(activeSecs ?? 0)}` : 'Record new meeting';
}

export const WHATS_NEW_SEEN_KEY = 'whats-new-seen';

export function whatsNewUnseen(seen: string | null, version: string | null): boolean {
  return !!version && seen !== version;
}

export const SIDEBAR_COLLAPSED_KEY = 'conclave.sidebarCollapsed';

// Nothing stored (first launch, or an upgrade): expanded from 1600 px wide, collapsed below.
export function readSidebarCollapsed(width: number): boolean {
  try {
    const stored = localStorage.getItem(SIDEBAR_COLLAPSED_KEY);
    return stored === null ? width < 1600 : stored === 'true';
  } catch {
    return width < 1600;
  }
}

export function writeSidebarCollapsed(collapsed: boolean): void {
  try {
    localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(collapsed));
  } catch {
    // storage unavailable: the state stays in memory only
  }
}
