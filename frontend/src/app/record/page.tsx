'use client';

import { useState, useEffect } from 'react';
import { RecordingControls } from '@/components/RecordingControls';
import { MicFollowingLine } from '@/components/MicFollowingLine';
import { usePermissionCheck } from '@/hooks/usePermissionCheck';
import { useRecordingState, RecordingStatus } from '@/contexts/RecordingStateContext';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useConfig } from '@/contexts/ConfigContext';
import { StatusOverlays } from '@/app/_components/StatusOverlays';
import { StartupChecks } from '@/app/_components/StartupChecks';
import { SettingsModals } from '@/app/_components/SettingsModal';
import { LiveTranscriptAndNotes } from '@/app/_components/LiveTranscriptAndNotes';
import { useModalState } from '@/hooks/useModalState';
import { useRecordingStart } from '@/hooks/useRecordingStart';
import { useRecordingStop } from '@/hooks/useRecordingStop';

export default function RecordPage() {
  // Local page state (not moved to contexts)
  const [isRecording, setIsRecordingState] = useState(false);
  const [barHeights, setBarHeights] = useState(['58%', '76%', '58%']);

  // Use contexts for state management
  const { meetingTitle } = useTranscripts();
  const { transcriptModelConfig, selectedDevices } = useConfig();
  const recordingState = useRecordingState();

  // Extract status from global state
  const { status, isStopping, isProcessing, setStatus } = recordingState;

  // Hooks
  const { hasMicrophone } = usePermissionCheck();
  const { modals, messages, showModal, hideModal } = useModalState(transcriptModelConfig);
  const [isRecordingDisabled, setIsRecordingDisabled] = useState(false);
  const { handleRecordingStart } = useRecordingStart(isRecording, setIsRecordingState, showModal);

  // Get handleRecordingStop function and setIsStopping (state comes from global context)
  const { handleRecordingStop, setIsStopping } = useRecordingStop(
    setIsRecordingState,
    setIsRecordingDisabled
  );

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
      {/* Temporary: the startup checks also run here, so a reload after auto-start still offers recovery */}
      <StartupChecks />
      {/* All Modals supported*/}
      <SettingsModals
        modals={modals}
        messages={messages}
        onClose={hideModal}
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
