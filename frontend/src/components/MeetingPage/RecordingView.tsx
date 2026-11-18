'use client';

import { useState } from 'react';
import { RecordingControls } from '@/components/RecordingControls';
import { StatusOverlays } from '@/app/_components/StatusOverlays';
import { LiveTranscriptAndNotes } from '@/app/_components/LiveTranscriptAndNotes';
import { MeetingHeader } from '@/components/MeetingPage/MeetingHeader';
import type { MeetingRow } from '@/lib/meetings';
import { AudioLine } from '@/components/MeetingPage/AudioLine';
import { useAiSidebar } from '@/components/AiSidebar/AiSidebarProvider';
import { useRecordingState, RecordingStatus } from '@/contexts/RecordingStateContext';
import { useConfig } from '@/contexts/ConfigContext';
import { useRecordingStop } from '@/hooks/useRecordingStop';
import { formatRecordingTime } from '@/lib/meeting-notes';
import type { ModalType } from '@/hooks/useModalState';

/** The meeting page while this meeting records, and while its stop is saved: Stop, live transcript, notes. */
export function RecordingView({ row, onChange, onTitleSaved, showModal }: { row: MeetingRow | null; onChange?: (row: MeetingRow) => void; onTitleSaved?: () => void; showModal: (name: ModalType, message?: string) => void }) {
  const meetingId = row?.id; // null: a recording whose routing failed, saved as a new meeting
  const { open: sidebarOpen } = useAiSidebar();
  const { selectedDevices } = useConfig();
  const { status, isRecording, isStopping, isProcessing, activeDuration, setStatus } = useRecordingState();
  const [, setRecordingFlag] = useState(false);
  const [isRecordingDisabled, setIsRecordingDisabled] = useState(false);
  // The one stop/save path: RecordingControls stops the backend, then this snapshots and saves.
  const { handleRecordingStop, setIsStopping } = useRecordingStop(setRecordingFlag, setIsRecordingDisabled);
  const isProcessingStop = status === RecordingStatus.PROCESSING_TRANSCRIPTS || isProcessing;
  const saving = status === RecordingStatus.SAVING || status === RecordingStatus.PROCESSING_TRANSCRIPTS;
  const failed = status === RecordingStatus.ERROR;

  return (
    <div className="flex h-screen flex-col bg-muted/50">
      <div className="border-b border-border bg-background px-6 pt-4">{row && onChange ? <MeetingHeader row={row} live onChange={onChange} onTitleSaved={onTitleSaved} /> : <h1 className="pb-3 text-xl font-semibold">New recording</h1>}</div>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-border bg-background px-6 py-3">
        <span className="flex items-center gap-2 text-sm text-foreground" role="status">
          {isRecording && <span aria-hidden className="h-2 w-2 rounded-full" style={{ backgroundColor: 'hsl(var(--rec))' }} />}
          {isRecording ? `Recording ${formatRecordingTime(activeDuration)}` : failed ? 'Not saved' : 'Saving…'}
        </span>
        {isRecording && (
          <RecordingControls
            labeled
            isRecording={isRecording}
            onRecordingStop={(callApi = true) => handleRecordingStop(callApi)}
            onRecordingStart={() => {}}
            onTranscriptReceived={() => {}}
            onStopInitiated={() => setIsStopping(true)}
            onStopFailed={() => setStatus(RecordingStatus.RECORDING)}
            barHeights={['4px', '4px', '4px']}
            onTranscriptionError={(message) => showModal('errorAlert', message)}
            isRecordingDisabled={isRecordingDisabled}
            isParentProcessing={isProcessingStop}
            selectedDevices={selectedDevices}
            meetingName={row?.title}
          />
        )}
        <div className="ml-auto"><AudioLine disabled /></div>
      </div>
      {failed && (
        <div role="alert" className="flex items-center gap-3 border-b border-border bg-background px-6 py-2 text-sm text-foreground">
          <span>The meeting was not saved. Its transcript and notes are still here.</span>
          <button type="button" onClick={() => { void handleRecordingStop(true); }} className="rounded-md border border-border px-3 py-1 hover:bg-accent">
            Try saving again
          </button>
        </div>
      )}
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <LiveTranscriptAndNotes tabbed={sidebarOpen} meetingId={meetingId} isProcessingStop={isProcessingStop} isStopping={isStopping} showModal={showModal}>
          <StatusOverlays isProcessing={status === RecordingStatus.PROCESSING_TRANSCRIPTS && !isRecording} isSaving={status === RecordingStatus.SAVING} />
        </LiveTranscriptAndNotes>
      </div>
    </div>
  );
}
