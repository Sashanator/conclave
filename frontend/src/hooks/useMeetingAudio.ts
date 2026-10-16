'use client';
import { useCallback, useEffect, useRef, useState } from 'react';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { AUDIO_DELETED_EVENT, type MeetingAudioInfo } from '@/lib/meeting-audio';

export type PlaybackRate = 1 | 1.5 | 2;

export function useMeetingAudio(meetingId: string) {
  const [info, setInfo] = useState<MeetingAudioInfo | null>(null);
  const audioRef = useRef<HTMLAudioElement>(null);
  const [playhead, setPlayhead] = useState<number | null>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [duration, setDuration] = useState<number | null>(null);
  const [rate, setRateState] = useState<PlaybackRate>(1);

  useEffect(() => {
    let cancelled = false;
    const load = () =>
      invoke<MeetingAudioInfo>('get_meeting_audio', { meetingId })
        .then((i) => { if (!cancelled) setInfo(i); })
        .catch((e) => console.error('Failed to locate meeting audio:', e));
    void load();
    // The old-audio review (mounted in the layout) may delete this meeting's audio while it is open.
    window.addEventListener(AUDIO_DELETED_EVENT, load);
    return () => { cancelled = true; window.removeEventListener(AUDIO_DELETED_EVENT, load); };
  }, [meetingId]);

  const src = info?.path ? convertFileSrc(info.path) : null;

  const playFrom = useCallback((seconds: number) => {
    const el = audioRef.current;
    if (!el) return;
    el.currentTime = seconds;
    void el.play().catch((e) => toast.error('Could not play the recording', { description: String(e) }));
  }, []);
  const togglePlay = useCallback(() => {
    const el = audioRef.current;
    if (!el) return;
    if (el.paused) void el.play().catch((e) => toast.error('Could not play the recording', { description: String(e) }));
    else el.pause();
  }, []);
  const seek = useCallback((seconds: number) => { if (audioRef.current) audioRef.current.currentTime = seconds; }, []);
  const setRate = useCallback((r: PlaybackRate) => { setRateState(r); if (audioRef.current) audioRef.current.playbackRate = r; }, []);
  const setKeepAudio = useCallback(async (keep: boolean) => {
    try {
      await invoke('set_keep_audio', { meetingId, keep });
      setInfo((i) => (i ? { ...i, keep_audio: keep } : i));
    } catch (e) {
      toast.error('Could not change Keep audio', { description: String(e) });
    }
  }, [meetingId]);

  return {
    info, src, audioRef, playhead, duration, isPlaying, rate, setRate, playFrom, togglePlay, seek, setKeepAudio,
    // spread onto the single <audio> element in page-content.tsx
    audioEvents: {
      onLoadedMetadata: (e: React.SyntheticEvent<HTMLAudioElement>) => setDuration(Number.isFinite(e.currentTarget.duration) ? e.currentTarget.duration : null),
      onDurationChange: (e: React.SyntheticEvent<HTMLAudioElement>) => setDuration(Number.isFinite(e.currentTarget.duration) ? e.currentTarget.duration : null),
      onTimeUpdate: (e: React.SyntheticEvent<HTMLAudioElement>) => setPlayhead(e.currentTarget.currentTime),
      onPlay: () => setIsPlaying(true),
      onPause: () => setIsPlaying(false),
      onEnded: () => setIsPlaying(false),
    },
  };
}
