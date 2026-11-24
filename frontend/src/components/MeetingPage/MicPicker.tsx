'use client';

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import type { AudioDevice } from '@/components/DeviceSelection';
import { useRecordingState } from '@/contexts/RecordingStateContext';

/** The mic the recording captures now, asked from the backend again after every swap event. */
export function useLiveMic(): [string | null, () => void] {
  const { micFollowing } = useRecordingState();
  const [mic, setMic] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    invoke<string | null>('get_recording_mic').then((m) => setMic(m ?? null)).catch((err) => console.error('Could not read the recording mic:', err));
  }, [micFollowing.device, tick]);
  return [mic, () => setTick((t) => t + 1)];
}

/** Lists the input devices and switches the live recording mic. `value` is the mic being recorded. */
export function MicPicker({ value, onSwitched, id }: { value: string | null; onSwitched?: () => void; id?: string }) {
  const [inputs, setInputs] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const { isRecording } = useRecordingState();
  const recordingNow = useRef(isRecording);
  recordingNow.current = isRecording;
  const load = () => {
    invoke<AudioDevice[]>('get_audio_devices')
      .then((d) => setInputs((d ?? []).filter((x) => x.device_type === 'Input').map((x) => x.name)))
      .catch((err) => console.error('Could not list microphones:', err));
  };
  useEffect(load, []);
  const pick = async (name: string) => {
    setBusy(true);
    try {
      await invoke('switch_recording_mic', { deviceName: name });
      onSwitched?.();
    } catch (err) {
      // A stop during the switch ends the session; that is not an error to show.
      if (recordingNow.current) toast.error(String(err));
    } finally {
      setBusy(false);
    }
  };
  // The live mic may be missing from the list (default or just unplugged): keep it selectable so the trigger shows it.
  const names = value && !inputs.includes(value) ? [value, ...inputs] : inputs;
  return (
    <Select value={value ?? undefined} onValueChange={pick} onOpenChange={(o) => o && load()} disabled={busy || !isRecording}>
      <SelectTrigger id={id} aria-label="Microphone for this recording" className="h-8 w-[260px] max-w-full text-sm">
        <SelectValue placeholder="Microphone" />
      </SelectTrigger>
      <SelectContent>
        {names.map((n) => <SelectItem key={n} value={n}>{n}</SelectItem>)}
      </SelectContent>
    </Select>
  );
}
