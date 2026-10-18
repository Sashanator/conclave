import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { isJobBusy, type MeetingSpeakers } from '@/lib/speakers';

type NameTarget = { personId: string } | { newName: string };

// Speaker state of one meeting. Inactive (feature off): no commands and no events.
// onTranscriptsChanged re-fetches the transcript list after a publish;
// onLineMoved patches one line's speaker in place, so the list keeps its scroll position.
export function useMeetingSpeakers(
  meetingId: string,
  active: boolean,
  onTranscriptsChanged?: () => Promise<void> | void,
  onLineMoved?: (transcriptId: string, idx: number | null) => void,
) {
  const [data, setData] = useState<MeetingSpeakers | null>(null);
  const [livePercent, setLivePercent] = useState<number | null>(null);
  const [answered, setAnswered] = useState(false);
  const [namesChanged, setNamesChanged] = useState(false);
  const meetingRef = useRef(meetingId);
  // Replies are applied only when they answer the latest request
  const seqRef = useRef(0);
  const onChangedRef = useRef(onTranscriptsChanged);
  const onMovedRef = useRef(onLineMoved);
  useEffect(() => { onChangedRef.current = onTranscriptsChanged; }, [onTranscriptsChanged]);
  useEffect(() => { onMovedRef.current = onLineMoved; }, [onLineMoved]);

  // A reply for a meeting we have left is dropped
  useEffect(() => {
    meetingRef.current = meetingId;
    seqRef.current += 1;
    setData(null);
    setLivePercent(null);
    setAnswered(false);
    setNamesChanged(false);
  }, [meetingId]);

  const reload = useCallback(async () => {
    const id = meetingId;
    const seq = ++seqRef.current;
    try {
      const next = await invoke<MeetingSpeakers>('speakers_get_meeting', { meetingId: id });
      if (meetingRef.current === id && seq === seqRef.current) {
        setData(next);
        setLivePercent(null);
      }
    } catch (err) {
      console.error('Failed to load speakers:', err);
    } finally {
      // Only the latest request opens the gate; a late reply to an older one must not
      if (meetingRef.current === id && seq === seqRef.current) setAnswered(true);
    }
  }, [meetingId]);

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    // Each listener has its own catch, so one that fails does not stop the other from being cleaned up
    const pending = [
      listen<{ meeting_id: string }>('speakers-updated', (event) => {
        if (event.payload.meeting_id !== meetingId) return;
        void reload();
        void onChangedRef.current?.();
      }),
      listen<{ meeting_id: string; percent: number }>('speakers-progress', (event) => {
        if (event.payload.meeting_id === meetingId) setLivePercent(event.payload.percent);
      }),
    ].map((p) => p.catch((err) => {
      console.error('Failed to listen for speaker events:', err);
      return null;
    }));
    // The first read waits for the listeners, so a terminal event sent meanwhile is not lost
    void Promise.all(pending).then(() => { if (!cancelled) void reload(); });
    return () => {
      cancelled = true;
      for (const p of pending) void p.then((unlisten) => unlisten?.());
    };
  }, [active, meetingId, reload]);

  const identify = useCallback(async () => {
    try {
      await invoke('speakers_identify', { meetingId });
    } catch (err) {
      toast.error('Could not start speaker analysis', { description: String(err) });
    }
    await reload();
  }, [meetingId, reload]);

  // Resolves to the person's id, or null when the rename failed
  const name = useCallback(async (idx: number, target: NameTarget): Promise<string | null> => {
    let personId: string | null = null;
    try {
      personId = await invoke<string>('speakers_name', { meetingId, idx, ...target });
      setNamesChanged(true);
    } catch (err) {
      toast.error('Could not rename the speaker', { description: String(err) });
    }
    await reload();
    return personId;
  }, [meetingId, reload]);

  const moveLine = useCallback(async (transcriptId: string, idx: number | null) => {
    try {
      await invoke('speakers_move_line', { transcriptId, idx });
      setNamesChanged(true);
      onMovedRef.current?.(transcriptId, idx);
    } catch (err) {
      toast.error('Could not move the line', { description: String(err) });
    }
    await reload();
  }, [meetingId, reload]);

  return {
    data: active ? data : null,
    loaded: !active || answered,
    busy: active && isJobBusy(data?.job ?? null),
    percent: livePercent ?? data?.job?.percent ?? null,
    identify,
    name,
    moveLine,
    namesChanged,
    clearNamesChanged: () => setNamesChanged(false),
  };
}

export type MeetingSpeakersState = ReturnType<typeof useMeetingSpeakers>;
