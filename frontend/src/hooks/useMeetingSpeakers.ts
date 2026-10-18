import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { isJobBusy, type MeetingSpeakers } from '@/lib/speakers';

type NameTarget = { personId: string } | { newName: string };

// Speaker state of one meeting. Inactive (feature off): no commands and no events.
// onTranscriptsChanged re-fetches the transcript list after a publish or a line move.
export function useMeetingSpeakers(
  meetingId: string,
  active: boolean,
  onTranscriptsChanged?: () => Promise<void> | void,
) {
  const [data, setData] = useState<MeetingSpeakers | null>(null);
  const [answered, setAnswered] = useState(false);
  const [namesChanged, setNamesChanged] = useState(false);
  const meetingRef = useRef(meetingId);
  const onChangedRef = useRef(onTranscriptsChanged);
  useEffect(() => { onChangedRef.current = onTranscriptsChanged; }, [onTranscriptsChanged]);

  // A reply for a meeting we have left is dropped
  useEffect(() => {
    meetingRef.current = meetingId;
    setData(null);
    setAnswered(false);
    setNamesChanged(false);
  }, [meetingId]);

  const reload = useCallback(async () => {
    const id = meetingId;
    try {
      const next = await invoke<MeetingSpeakers>('speakers_get_meeting', { meetingId: id });
      if (meetingRef.current === id) setData(next);
    } catch (err) {
      console.error('Failed to load speakers:', err);
    } finally {
      if (meetingRef.current === id) setAnswered(true);
    }
  }, [meetingId]);

  useEffect(() => {
    if (!active) return;
    void reload();
    const unlisten = Promise.all([
      listen<{ meeting_id: string }>('speakers-updated', (event) => {
        if (event.payload.meeting_id !== meetingId) return;
        void reload();
        void onChangedRef.current?.();
      }),
      listen<{ meeting_id: string; percent: number }>('speakers-progress', (event) => {
        if (event.payload.meeting_id === meetingId) void reload();
      }),
    ]);
    return () => {
      void unlisten.then((fns) => fns.forEach((fn) => fn()));
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

  const name = useCallback(async (idx: number, target: NameTarget) => {
    try {
      await invoke('speakers_name', { meetingId, idx, ...target });
      setNamesChanged(true);
    } catch (err) {
      toast.error('Could not rename the speaker', { description: String(err) });
    }
    await reload();
  }, [meetingId, reload]);

  const moveLine = useCallback(async (transcriptId: string, idx: number | null) => {
    try {
      await invoke('speakers_move_line', { transcriptId, idx });
      setNamesChanged(true);
      // The line's label comes from the transcript list, so it has to be re-read
      await onChangedRef.current?.();
    } catch (err) {
      toast.error('Could not move the line', { description: String(err) });
    }
    await reload();
  }, [meetingId, reload]);

  return {
    data: active ? data : null,
    loaded: !active || answered,
    busy: active && isJobBusy(data?.job ?? null),
    percent: data?.job?.percent ?? null,
    identify,
    name,
    moveLine,
    namesChanged,
    clearNamesChanged: () => setNamesChanged(false),
  };
}

export type MeetingSpeakersState = ReturnType<typeof useMeetingSpeakers>;
