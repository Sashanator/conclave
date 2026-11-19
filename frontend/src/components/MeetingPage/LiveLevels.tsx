'use client';

import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { CompactAudioLevelMeter } from '@/components/AudioLevelMeter';
import type { AudioLevelUpdate } from '@/components/DeviceSelection';

const SILENT = { rms_level: 0, peak_level: 0, is_active: false };

/** The real microphone and system levels while recording. Decorative: the timer carries the state. */
export function LiveLevels() {
  const [levels, setLevels] = useState({ input: SILENT, output: SILENT });
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    listen<AudioLevelUpdate>('audio-levels', (e) => {
      const pick = (type: string) => e.payload.levels.find((l) => l.device_type === type) ?? SILENT;
      setLevels({ input: pick('input'), output: pick('output') });
    })
      .then((u) => { if (gone) u(); else unlisten = u; })
      .catch((err) => console.error('Could not listen for audio levels:', err));
    return () => { gone = true; unlisten?.(); };
  }, []);
  const meter = (label: string, l: typeof SILENT) => (
    <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
      {label}
      <CompactAudioLevelMeter rmsLevel={l.rms_level} peakLevel={l.peak_level} isActive={l.is_active} />
    </span>
  );
  return (
    <div aria-hidden className="flex items-center gap-3">
      {meter('Mic', levels.input)}
      {meter('System', levels.output)}
    </div>
  );
}
