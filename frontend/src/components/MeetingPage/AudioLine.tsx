'use client';

import { DeviceSelection } from '@/components/DeviceSelection';
import { LanguageSelection } from '@/components/LanguageSelection';
import { MicFollowingLine } from '@/components/MicFollowingLine';
import { useConfig } from '@/contexts/ConfigContext';

/** What the next recording will capture: microphone, system audio and language (existing pickers). */
export function AudioLine({ disabled }: { disabled: boolean }) {
  const { selectedDevices, setSelectedDevices, selectedLanguage, setSelectedLanguage, transcriptModelConfig } = useConfig();
  return (
    <div className="space-y-3">
      <DeviceSelection selectedDevices={selectedDevices} onDeviceChange={setSelectedDevices} disabled={disabled} />
      <LanguageSelection
        selectedLanguage={selectedLanguage}
        onLanguageChange={setSelectedLanguage}
        disabled={disabled}
        provider={transcriptModelConfig.provider}
      />
      <MicFollowingLine selectedMic={selectedDevices?.micDevice ?? null} />
    </div>
  );
}
