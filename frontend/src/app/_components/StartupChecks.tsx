'use client';

import { useState, useEffect, useRef } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { useRecordingState, RecordingStatus } from '@/contexts/RecordingStateContext';
import { useTranscriptRecovery } from '@/hooks/useTranscriptRecovery';
import { TranscriptRecovery } from '@/components/TranscriptRecovery';
import { indexedDBService } from '@/services/indexedDBService';
import { recoveredDescription } from '@/lib/recovery-toast';

// Module-level so the CUDA warning is shown once per app session, not on every remount.
let cudaToastChecked = false;

export function StartupChecks() {
  const [showRecoveryDialog, setShowRecoveryDialog] = useState(false);
  const navTimer = useRef<ReturnType<typeof setTimeout>>();
  useEffect(() => () => clearTimeout(navTimer.current), []);
  const router = useRouter();
  const recordingState = useRecordingState();
  const { status } = recordingState;

  const {
    recoverableMeetings,
    checkForRecoverableTranscripts,
    recoverMeeting,
    loadMeetingTranscripts,
    deleteRecoverableMeeting
  } = useTranscriptRecovery();

  // Warn once at startup when a CUDA build cannot find its runtime and Whisper falls back to CPU
  useEffect(() => {
    if (cudaToastChecked) return;
    cudaToastChecked = true;
    invoke<{ label: string; cuda_runtime_missing: boolean }>('get_whisper_backend_status')
      .then((s) => {
        if (s.cuda_runtime_missing) {
          toast.warning('CUDA 12 runtime not found — Whisper runs on CPU. See CUDA_SETUP.md', {
            duration: 10000,
            action: {
              label: 'Open guide',
              onClick: () => invoke('open_external_url', { url: 'https://github.com/Sashanator/conclave/blob/main/CUDA_SETUP.md' })
            }
          });
        }
      })
      .catch(() => {});
  }, []);

  // The old-meeting upgrade at startup failed: Rust hands the error out once, so a remount shows nothing.
  useEffect(() => {
    invoke<string | null>('take_backfill_error')
      .then((err) => {
        if (err) toast.error("Some old meetings couldn't be upgraded; Conclave will retry at next start.", { description: err });
      })
      .catch((e) => console.error('Could not read the startup upgrade status:', e));
  }, []);

  // Startup recovery check
  useEffect(() => {
    const performStartupChecks = async () => {
      try {
        // Skip recovery check if currently recording or processing stop
        // This prevents the recovery dialog from showing when:
        if (recordingState.isRecording ||
          status === RecordingStatus.STOPPING ||
          status === RecordingStatus.PROCESSING_TRANSCRIPTS ||
          status === RecordingStatus.SAVING) {
          console.log('Skipping recovery check - recording in progress or processing');
          return;
        }

        // 1. Clean up old meetings (7+ days)
        try {
          await indexedDBService.deleteOldMeetings(7);
        } catch (error) {
          console.warn('⚠️ Failed to clean up old meetings:', error);
        }

        // 2. Clean up saved meetings (24+ hours after save)
        try {
          await indexedDBService.deleteSavedMeetings(24);
        } catch (error) {
          console.warn('⚠️ Failed to clean up saved meetings:', error);
        }

        // 3. Always check for recoverable meetings on startup
        // Don't skip based on sessionStorage - we need to check every time
        await checkForRecoverableTranscripts();
      } catch (error) {
        console.error('Failed to perform startup checks:', error);
      }
    };

    performStartupChecks();
  }, [checkForRecoverableTranscripts, recordingState.isRecording, status]);

  // Watch for recoverable meetings changes and show dialog once per session
  useEffect(() => {
    // Only show dialog if we have meetings and haven't shown it yet this session
    if (recoverableMeetings.length > 0) {
      const shownThisSession = sessionStorage.getItem('recovery_dialog_shown');
      if (!shownThisSession) {
        setShowRecoveryDialog(true);
        sessionStorage.setItem('recovery_dialog_shown', 'true');
      }
    }
  }, [recoverableMeetings]);

  // Handle recovery with toast notifications and navigation
  const handleRecovery = async (meetingId: string) => {
    try {
      const result = await recoverMeeting(meetingId);

      if (result.success) {
        toast.success('Meeting recovered successfully!', {
          description: recoveredDescription(result.audioRecoveryStatus?.status),
          action: result.meetingId ? {
            label: 'View Meeting',
            onClick: () => {
              router.push(`/meeting-details?id=${result.meetingId}`);
            }
          } : undefined,
          duration: 10000,
        });

        // If no more recoverable meetings, clear session flag so dialog can show again
        if (recoverableMeetings.length === 0) {
          sessionStorage.removeItem('recovery_dialog_shown');
        }

        // Auto-navigate after a short delay
        if (result.meetingId) {
          navTimer.current = setTimeout(() => {
            router.push(`/meeting-details?id=${result.meetingId}`);
          }, 2000);
        }
      }
    } catch (error) {
      toast.error('Failed to recover meeting', {
        description: error instanceof Error ? error.message : 'Unknown error occurred',
      });
      throw error;
    }
  };

  // Handle dialog close - clear session flag if no meetings left
  const handleDialogClose = () => {
    setShowRecoveryDialog(false);
    // If user closes dialog and there are no more meetings, clear the flag
    // This allows the dialog to show again next session if new meetings appear
    if (recoverableMeetings.length === 0) {
      sessionStorage.removeItem('recovery_dialog_shown');
    }
  };

  return (
    <TranscriptRecovery
      isOpen={showRecoveryDialog}
      onClose={handleDialogClose}
      recoverableMeetings={recoverableMeetings}
      onRecover={handleRecovery}
      onDelete={deleteRecoverableMeeting}
      onLoadPreview={loadMeetingTranscripts}
    />
  );
}
