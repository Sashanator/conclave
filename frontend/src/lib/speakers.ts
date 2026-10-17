// Pure helpers for speaker labels in the transcript and meeting UI

import type { MeetingSpeakers, SpeakerSettings, SpeakerView } from '../types';

export type { MeetingSpeakers, SpeakerJobStatus, SpeakerSettings, SpeakerView } from '../types';

const BACKEND_LABELS: Record<string, string> = { directml: 'DirectML', cpu: 'CPU', mixed: 'Mixed' };

export function isJobBusy(job: MeetingSpeakers['job']): boolean {
  return job?.status === 'pending' || job?.status === 'running';
}

// Before analysis only the live labels exist: idx 0 is the microphone-only "You"
export function lineSpeakerName(idx: number | null | undefined, speakers: SpeakerView[], analysed: boolean): string {
  if (!analysed) return idx === 0 ? 'You' : 'Others';
  if (idx == null) return 'Unknown';
  return speakers.find((s) => s.idx === idx)?.name ?? 'Unknown';
}

// One chip per person: unnamed speakers (no person_id) stay separate; self first, then by talk time
export function speakerChips(speakers: SpeakerView[]): { key: string; name: string; idxs: number[]; seconds: number; isSelf: boolean }[] {
  const chips = new Map<string, { key: string; name: string; idxs: number[]; seconds: number; isSelf: boolean }>();
  for (const s of speakers) {
    const key = s.person_id ?? `idx-${s.idx}`;
    const chip = chips.get(key);
    if (chip) {
      chip.idxs.push(s.idx);
      chip.seconds += s.seconds;
      chip.isSelf = chip.isSelf || s.is_self;
    } else {
      chips.set(key, { key, name: s.name, idxs: [s.idx], seconds: s.seconds, isSelf: s.is_self });
    }
  }
  return [...chips.values()].sort((a, b) =>
    a.isSelf !== b.isSelf ? (a.isSelf ? -1 : 1) : b.seconds - a.seconds
  );
}

export function formatTalkTime(seconds: number): string {
  const total = Number.isFinite(seconds) ? Math.max(0, Math.floor(seconds)) : 0;
  if (total < 60) return `${total}s`;
  const minutes = Math.floor(total / 60);
  if (minutes < 60) return `${minutes}m`;
  return `${Math.floor(minutes / 60)}h ${String(minutes % 60).padStart(2, '0')}m`;
}

export function lastAnalysisText(a: SpeakerSettings['last_analysis']): string | null {
  if (!a) return null;
  const backend = BACKEND_LABELS[a.backend] ?? a.backend;
  const meetingMinutes = Math.max(1, Math.round(a.audio_seconds / 60));
  return `Last analysis: ${backend} · ${Math.round(a.run_seconds)} s for a ${meetingMinutes}-min meeting`;
}
