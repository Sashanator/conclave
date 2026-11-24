'use client';

import { useConfig } from '@/contexts/ConfigContext';
import { audioSummary } from '@/lib/meeting-page';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { AudioLine } from './AudioLine';
import { MicPicker, useLiveMic } from './MicPicker';

/** One line saying what the next recording captures; Change opens the device and language pickers. */
export function AudioSummaryLine({ changeable, live = false }: { changeable: boolean; live?: boolean }) {
  const { selectedDevices, selectedLanguage, allowedLanguages, transcriptModelConfig } = useConfig();
  const summary = audioSummary({
    micDevice: selectedDevices?.micDevice ?? null,
    systemDevice: selectedDevices?.systemDevice ?? null,
    language: selectedLanguage,
    allowed: allowedLanguages,
    provider: transcriptModelConfig.provider,
  });
  if (live) return <LiveMicLine rest={summary.split(' · ').slice(1).join(' · ')} />;
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

/** While recording: the Mic part is a picker for the mic actually recorded; the rest stays the summary. */
function LiveMicLine({ rest }: { rest: string }) {
  const [liveMic, refreshLiveMic] = useLiveMic();
  return (
    <p className="flex flex-wrap items-center gap-x-2 text-sm text-muted-foreground">
      <label htmlFor="live-mic">Mic:</label>
      <MicPicker id="live-mic" value={liveMic} onSwitched={refreshLiveMic} />
      <span>· {rest}</span>
    </p>
  );
}
