import { invoke } from '@tauri-apps/api/core';
import { normaliseLanguageCode } from '@/lib/summary-languages';

export const SUMMARY_LANGUAGE_RECENTS_KEY = 'summaryLanguageRecents';
export const SUMMARY_LANGUAGE_DEFAULT_KEY = 'summaryLanguageDefault';

export function readPinnedSummaryLanguageDefault(): string | null {
  if (typeof window === 'undefined') return null;
  try {
    return normaliseLanguageCode(window.localStorage.getItem(SUMMARY_LANGUAGE_DEFAULT_KEY));
  } catch {
    return null;
  }
}

export function writePinnedSummaryLanguageDefault(value: string | null): void {
  if (typeof window === 'undefined') return;
  try {
    if (value) window.localStorage.setItem(SUMMARY_LANGUAGE_DEFAULT_KEY, value);
    else window.localStorage.removeItem(SUMMARY_LANGUAGE_DEFAULT_KEY);
  } catch {
    // Preference writes are non-critical; meeting-specific persistence happens separately.
  }
}

export async function applyPinnedSummaryLanguageToMeeting(meetingId: string): Promise<string | null> {
  const pinned = readPinnedSummaryLanguageDefault();
  if (!pinned) return null;

  await invoke('api_save_meeting_summary_language', {
    meetingId,
    summaryLanguage: pinned,
  });

  return pinned;
}
