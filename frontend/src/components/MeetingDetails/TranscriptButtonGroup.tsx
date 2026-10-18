"use client";

import { useState, useCallback } from 'react';
import { Button } from '@/components/ui/button';
import { ButtonGroup } from '@/components/ui/button-group';
import { Copy, FolderOpen, RefreshCw, Users } from 'lucide-react';
import { RetranscribeDialog } from './RetranscribeDialog';
import { useConfig } from '@/contexts/ConfigContext';
import type { MeetingSpeakers } from '@/lib/speakers';


interface TranscriptButtonGroupProps {
  transcriptCount: number;
  onCopyTranscript: () => void;
  onOpenMeetingFolder: () => Promise<void>;
  meetingId?: string;
  meetingFolderPath?: string | null;
  onRefetchTranscripts?: () => Promise<void>;
  /** Why the audio cannot be re-transcribed; disables Enhance and explains why on hover. */
  audioMissingReason?: string | null;
  /** Speaker analysis job of this meeting; Re-identify once it is done */
  speakerJob?: MeetingSpeakers['job'];
  /** Labels already exist, so Identify asks before replacing them */
  hasSpeakers?: boolean;
  speakersBusy?: boolean;
  onIdentifySpeakers?: () => Promise<void>;
}


export function TranscriptButtonGroup({
  transcriptCount,
  onCopyTranscript,
  onOpenMeetingFolder,
  meetingId,
  meetingFolderPath,
  onRefetchTranscripts,
  audioMissingReason,
  speakerJob,
  hasSpeakers = false,
  speakersBusy = false,
  onIdentifySpeakers,
}: TranscriptButtonGroupProps) {
  const { betaFeatures, speakerIdActive } = useConfig();
  const reidentify = speakerJob?.status === 'done';
  const replacesLabels = reidentify || hasSpeakers;
  const [showRetranscribeDialog, setShowRetranscribeDialog] = useState(false);

  const handleIdentify = () => {
    if (replacesLabels && !window.confirm('Labels, including your manual fixes, will be replaced. Continue?')) return;
    void onIdentifySpeakers?.();
  };

  const handleRetranscribeComplete = useCallback(async () => {
    // Refetch transcripts to show the updated data
    if (onRefetchTranscripts) {
      await onRefetchTranscripts();
    }
  }, [onRefetchTranscripts]);

  return (
    <div className="flex items-center justify-center w-full gap-2">
      <ButtonGroup>
        <Button
          variant="outline"
          size="sm"
          className="px-2 @[22rem]:px-3"
          onClick={() => {
            onCopyTranscript();
          }}
          disabled={transcriptCount === 0}
          title={transcriptCount === 0 ? 'No transcript available' : 'Copy Transcript'}
        >
          <Copy />
          <span className="hidden @[22rem]:inline">Copy</span>
        </Button>

        <Button
          size="sm"
          variant="outline"
          className="px-2 @[22rem]:px-4"
          onClick={() => {
            onOpenMeetingFolder();
          }}
          title="Open Recording Folder"
        >
          <FolderOpen className="@[22rem]:mr-2" size={18} />
          <span className="hidden @[22rem]:inline">Recording</span>
        </Button>

        {speakerIdActive && meetingFolderPath && !audioMissingReason && onIdentifySpeakers && (
          <Button
            size="sm"
            variant="outline"
            className="px-2 @[22rem]:px-3"
            onClick={handleIdentify}
            disabled={speakersBusy}
            title={reidentify ? 'Re-identify' : 'Identify speakers'}
          >
            <Users />
            <span className="hidden @[22rem]:inline">{reidentify ? 'Re-identify' : 'Identify speakers'}</span>
          </Button>
        )}

        {betaFeatures.importAndRetranscribe && meetingId && meetingFolderPath && (
          <Button
            size="sm"
            variant="outline"
            className="bg-gradient-to-r from-blue-50 to-purple-50 dark:from-blue-950/40 dark:to-purple-950/40 hover:from-blue-100 hover:to-purple-100 dark:hover:from-blue-900/50 dark:hover:to-purple-900/50 border-blue-200 dark:border-blue-800 px-2 @[22rem]:px-4"
            onClick={() => {
              setShowRetranscribeDialog(true);
            }}
            disabled={transcriptCount === 0 || !!audioMissingReason}
            title={audioMissingReason ?? 'Retranscribe to enhance your recorded audio'}
          >
            <RefreshCw className="@[22rem]:mr-2" size={18} />
            <span className="hidden @[22rem]:inline">Enhance</span>
          </Button>
        )}
      </ButtonGroup>

      {betaFeatures.importAndRetranscribe && meetingId && meetingFolderPath && (
        <RetranscribeDialog
          open={showRetranscribeDialog}
          onOpenChange={setShowRetranscribeDialog}
          meetingId={meetingId}
          meetingFolderPath={meetingFolderPath}
          onComplete={handleRetranscribeComplete}
        />
      )}
    </div>
  );
}
