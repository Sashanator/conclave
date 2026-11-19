'use client';

import { useConfig } from '@/contexts/ConfigContext';
import { audioSummary } from '@/lib/meeting-page';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { AudioLine } from './AudioLine';

/** One line saying what the next recording captures; Change opens the device and language pickers. */
export function AudioSummaryLine({ changeable }: { changeable: boolean }) {
  const { selectedDevices, selectedLanguage, allowedLanguages, transcriptModelConfig } = useConfig();
  const summary = audioSummary({
    micDevice: selectedDevices?.micDevice ?? null,
    systemDevice: selectedDevices?.systemDevice ?? null,
    language: selectedLanguage,
    allowed: allowedLanguages,
    provider: transcriptModelConfig.provider,
  });
  return (
    <p className="text-sm text-muted-foreground">
      {summary}
      {changeable && (
        <>
          {' · '}
          <Popover>
            <PopoverTrigger asChild>
              <button type="button" className="text-primary underline-offset-2 hover:underline focus-visible:ring-2 focus-visible:ring-ring rounded">Change</button>
            </PopoverTrigger>
            <PopoverContent align="start" className="w-[360px] max-w-[calc(100vw-2rem)]">
              <AudioLine disabled={false} />
            </PopoverContent>
          </Popover>
        </>
      )}
    </p>
  );
}
