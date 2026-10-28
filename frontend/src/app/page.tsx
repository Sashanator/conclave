'use client';

import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { RecordingControls } from '@/components/RecordingControls';
import { MicFollowingLine } from '@/components/MicFollowingLine';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { usePermissionCheck } from '@/hooks/usePermissionCheck';
import { useRecordingState, RecordingStatus } from '@/contexts/RecordingStateContext';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useConfig } from '@/contexts/ConfigContext';
import { StatusOverlays } from '@/app/_components/StatusOverlays';
import { SettingsModals } from './_components/SettingsModal';
import { LiveTranscriptAndNotes } from './_components/LiveTranscriptAndNotes';
import { useModalState } from '@/hooks/useModalState';
import { useRecordingStart } from '@/hooks/useRecordingStart';
import { useRecordingStop } from '@/hooks/useRecordingStop';
import { useTranscriptRecovery } from '@/hooks/useTranscriptRecovery';
import { TranscriptRecovery } from '@/components/TranscriptRecovery';
import { indexedDBService } from '@/services/indexedDBService';
import { toast } from 'sonner';
import { recoveredDescription } from '@/lib/recovery-toast';
import { useRouter } from 'next/navigation';

// Module-level so the CUDA warning is shown once per app session, not on every remount of Home.
let cudaToastChecked = false;

export default function Home() {
  // Local page state (not moved to contexts)
  const [isRecording, setIsRecordingState] = useState(false);
  const [barHeights, setBarHeights] = useState(['58%', '76%', '58%']);
  const [showRecoveryDialog, setShowRecoveryDialog] = useState(false);

  // Use contexts for state management
  const { meetingTitle } = useTranscripts();
  const { transcriptModelConfig, selectedDevices } = useConfig();
  const recordingState = useRecordingState();

  // Extract status from global state
  const { status, isStopping, isProcessing, isSaving, setStatus } = recordingState;

  // Hooks
  const { hasMicrophone } = usePermissionCheck();
  const { refetchMeetings } = useSidebar();
  const { modals, messages, showModal, hideModal } = useModalState(transcriptModelConfig);
  const [isRecordingDisabled, setIsRecordingDisabled] = useState(false);
  const { handleRecordingStart } = useRecordingStart(isRecording, setIsRecordingState, showModal);

  // Get handleRecordingStop function and setIsStopping (state comes from global context)
  const { handleRecordingStop, setIsStopping } = useRecordingStop(
    setIsRecordingState,
    setIsRecordingDisabled
  );

  // Recovery hook
  const {
    recoverableMeetings,
    isLoading: isLoadingRecovery,
    isRecovering,
    checkForRecoverableTranscripts,
    recoverMeeting,
    loadMeetingTranscripts,
    deleteRecoverableMeeting
  } = useTranscriptRecovery();

  const router = useRouter();

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

        // Refresh sidebar to show the newly recovered meeting
        await refetchMeetings();

        // If no more recoverable meetings, clear session flag so dialog can show again
        if (recoverableMeetings.length === 0) {
          sessionStorage.removeItem('recovery_dialog_shown');
        }

        // Auto-navigate after a short delay
        if (result.meetingId) {
          setTimeout(() => {
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

  useEffect(() => {
    if (recordingState.isRecording) {
      const interval = setInterval(() => {
        setBarHeights(prev => {
          const newHeights = [...prev];
          newHeights[0] = Math.random() * 20 + 10 + 'px';
          newHeights[1] = Math.random() * 20 + 10 + 'px';
          newHeights[2] = Math.random() * 20 + 10 + 'px';
          return newHeights;
        });
      }, 300);

      return () => clearInterval(interval);
    }
  }, [recordingState.isRecording]);

  // Computed values using global status
  const isProcessingStop = status === RecordingStatus.PROCESSING_TRANSCRIPTS || isProcessing;

  return (
    <div className="flex flex-col h-screen bg-muted/50">
      {/* All Modals supported*/}
      <SettingsModals
        modals={modals}
        messages={messages}
        onClose={hideModal}
      />

      {/* Recovery Dialog */}
      <TranscriptRecovery
        isOpen={showRecoveryDialog}
        onClose={handleDialogClose}
        recoverableMeetings={recoverableMeetings}
        onRecover={handleRecovery}
        onDelete={deleteRecoverableMeeting}
        onLoadPreview={loadMeetingTranscripts}
      />
      <div className="flex flex-1 overflow-hidden">
        <LiveTranscriptAndNotes
          isProcessingStop={isProcessingStop}
          isStopping={isStopping}
          showModal={showModal}
        >
          {/* Recording controls - only show when permissions are granted or already recording and not showing status messages */}
          {(hasMicrophone || isRecording) &&
            status !== RecordingStatus.PROCESSING_TRANSCRIPTS &&
            status !== RecordingStatus.SAVING && (
              <div className="absolute bottom-12 left-0 right-0 z-10">
                <div className="flex justify-center">
                  <div className="w-2/3 max-w-[750px] flex justify-center relative">
                    <div className="bg-card rounded-full shadow-lg flex items-center">
                      <RecordingControls
                        isRecording={recordingState.isRecording}
                        onRecordingStop={(callApi = true) => handleRecordingStop(callApi)}
                        onRecordingStart={handleRecordingStart}
                        onTranscriptReceived={() => { }} // Not actually used by RecordingControls
                        onStopInitiated={() => setIsStopping(true)}
                        onStopFailed={() => setStatus(RecordingStatus.RECORDING)}
                        barHeights={barHeights}
                        onTranscriptionError={(message) => {
                          showModal('errorAlert', message);
                        }}
                        isRecordingDisabled={isRecordingDisabled}
                        isParentProcessing={isProcessingStop}
                        selectedDevices={selectedDevices}
                        meetingName={meetingTitle}
                      />
                    </div>
                    {recordingState.isRecording && (
                      <div className="absolute top-full mt-2 left-0 right-0 flex justify-center">
                        <MicFollowingLine selectedMic={selectedDevices?.micDevice ?? null} />
                      </div>
                    )}
                  </div>
                </div>
              </div>
            )}

          {/* Status Overlays - Processing and Saving */}
          <StatusOverlays
            isProcessing={status === RecordingStatus.PROCESSING_TRANSCRIPTS && !recordingState.isRecording}
            isSaving={status === RecordingStatus.SAVING}
          />
        </LiveTranscriptAndNotes>
      </div>
    </div>
  );
}
