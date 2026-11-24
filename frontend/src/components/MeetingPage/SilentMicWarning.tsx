'use client';

import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { X } from 'lucide-react';
import type { AudioLevelUpdate } from '@/components/DeviceSelection';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { hasSound, micSilent } from '@/lib/silent-mic';
import { MicPicker, useLiveMic } from './MicPicker';

/** Inline warning when the live mic has been silent for 10 s. Dismissed once per mic; a switch restarts the timer. */
export function SilentMicWarning() {
  const { isPaused } = useRecordingState();
  const [mic, refreshMic] = useLiveMic();
  const [silent, setSilent] = useState(false);
  const [dismissed, setDismissed] = useState<string[]>([]);
  const lastSound = useRef(Date.now());
  const paused = useRef(isPaused);
  paused.current = isPaused;

  useEffect(() => { lastSound.current = Date.now(); setSilent(false); }, [mic]);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    listen<AudioLevelUpdate>('audio-levels', (e) => {
      const input = e.payload.levels.find((l) => l.device_type === 'input');
      if (input && hasSound(input.peak_level)) lastSound.current = Date.now();
    })
      .then((u) => { if (gone) u(); else unlisten = u; })
      .catch((err) => console.error('Could not listen for audio levels:', err));
    const timer = setInterval(() => {
      if (paused.current) lastSound.current = Date.now();
      setSilent(micSilent(lastSound.current, Date.now()));
    }, 1000);
    return () => { gone = true; unlisten?.(); clearInterval(timer); };
  }, []);

  if (!silent || !mic || dismissed.includes(mic)) return null;
  return (
    <div role="alert" className="flex flex-wrap items-center gap-3 border-b border-border bg-background py-2 text-sm text-foreground">
      <span>No sound from {mic}. Switch microphone?</span>
      <MicPicker value={mic} onSwitched={refreshMic} />
      <button type="button" aria-label="Dismiss" onClick={() => setDismissed((d) => [...d, mic])} className="ml-auto rounded p-1 hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring">
        <X className="h-4 w-4" aria-hidden />
      </button>
    </div>
  );
}
