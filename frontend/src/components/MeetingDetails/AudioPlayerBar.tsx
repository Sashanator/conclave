'use client';
import { Pause, Pin, Play } from 'lucide-react';
import { Switch } from '@/components/ui/switch';
import { audioMissingMessage } from '@/lib/meeting-audio';
import { formatRecordingTime } from '@/lib/meeting-notes';
import type { useMeetingAudio, PlaybackRate } from '@/hooks/useMeetingAudio';

const RATES: PlaybackRate[] = [1, 1.5, 2];

export function AudioPlayerBar({ audio }: { audio: ReturnType<typeof useMeetingAudio> }) {
  const missing = audioMissingMessage(audio.info);
  if (!audio.info) return null;
  const duration = audio.duration;
  return (
    <div className="flex items-center gap-3 border-t border-border bg-background px-3 py-2 text-sm">
      {missing ? (
        <span className="text-muted-foreground">{missing}</span>
      ) : (
        <>
          <button type="button" onClick={audio.togglePlay} aria-label={audio.isPlaying ? 'Pause' : 'Play'} className="rounded p-1 hover:bg-accent">
            {audio.isPlaying ? <Pause className="h-4 w-4" /> : <Play className="h-4 w-4" />}
          </button>
          <input
            type="range" min={0} max={duration ?? 0} step={0.1}
            value={audio.playhead ?? 0} onChange={(e) => audio.seek(Number(e.target.value))}
            aria-label="Playback position" className="flex-1 accent-blue-600"
          />
          <span className="tabular-nums text-muted-foreground">
            {formatRecordingTime(audio.playhead ?? 0)} / {formatRecordingTime(duration)}
          </span>
          <button type="button" className="rounded px-1.5 py-0.5 hover:bg-accent tabular-nums" aria-label={`Playback speed ${audio.rate}x`}
            onClick={() => audio.setRate(RATES[(RATES.indexOf(audio.rate) + 1) % RATES.length])}>
            {audio.rate}×
          </button>
        </>
      )}
      <label className="flex items-center gap-1.5 whitespace-nowrap" title="Leave this meeting out of the old-audio review">
        <Pin className="h-3.5 w-3.5" aria-hidden="true" />
        Keep audio
        <Switch checked={audio.info.keep_audio} onCheckedChange={(v) => void audio.setKeepAudio(v)} aria-label="Keep audio" />
      </label>
    </div>
  );
}
