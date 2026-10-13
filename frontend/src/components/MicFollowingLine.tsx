import { useMicFollowing } from '@/hooks/useMicFollowing';
import { micLine } from '@/lib/mic-following';

/** The live microphone under the recording pill, and the warning when following fails. */
export function MicFollowingLine({ selectedMic }: { selectedMic: string | null }) {
  const state = useMicFollowing();
  return (
    <div className="flex min-w-0 flex-col items-center">
      <p className="text-xs text-muted-foreground truncate">{micLine(state, selectedMic)}</p>
      {state.warning && <p className="text-xs text-amber-700 dark:text-amber-400">{state.warning}</p>}
    </div>
  );
}
