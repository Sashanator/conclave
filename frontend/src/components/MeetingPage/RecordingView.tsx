'use client';

import { useState } from 'react';
import { RecordingControls } from '@/components/RecordingControls';
import { StatusOverlays } from '@/app/_components/StatusOverlays';
import { LiveTranscriptAndNotes } from '@/app/_components/LiveTranscriptAndNotes';
import { MeetingHeader } from '@/components/MeetingPage/MeetingHeader';
import type { MeetingRow } from '@/lib/meetings';
import { AudioSummaryLine } from '@/components/MeetingPage/AudioSummaryLine';
import { SilentMicWarning } from '@/components/MeetingPage/SilentMicWarning';
import { LiveLevels } from '@/components/MeetingPage/LiveLevels';
import { BackCrumb } from '@/components/BackCrumb';
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

  const timer = (
    <span className="flex shrink-0 items-center gap-2 text-sm text-foreground" role="status">
      {isRecording && <span aria-hidden className="h-2 w-2 rounded-full" style={{ backgroundColor: 'hsl(var(--rec))' }} />}
      {isRecording ? <><span className="sr-only">Recording </span>{formatRecordingTime(activeDuration)}</> : failed ? 'Not saved' : 'Saving…'}
    </span>
  );
  const actions = (
    <div className="flex shrink-0 flex-nowrap items-center gap-4">
      {timer}
      {isRecording && <LiveLevels />}
      {isRecording && (
        <RecordingControls
          labeled
          isRecording={isRecording}
          onRecordingStop={(callApi = true) => handleRecordingStop(callApi)}
          onRecordingStart={() => {}}
          onTranscriptReceived={() => {}}
          onStopInitiated={() => setIsStopping(true)}
          onStopFailed={() => setStatus(RecordingStatus.RECORDING)}
          onTranscriptionError={(message) => showModal('errorAlert', message)}
          isRecordingDisabled={isRecordingDisabled}
          isParentProcessing={isProcessingStop}
          selectedDevices={selectedDevices}
          meetingName={row?.title}
        />
      )}
    </div>
  );

  return (
    <div className="flex h-screen flex-col bg-background px-8">
      <div className="pt-8 pb-2"><BackCrumb fallback="/" fallbackLabel="Meetings" /></div>
      {row && onChange ? (
        <MeetingHeader variant="compact" row={row} live onChange={onChange} onTitleSaved={onTitleSaved} actions={actions} />
      ) : (
        <div className="flex flex-nowrap items-center gap-4"><h1 className="min-w-0 flex-1 truncate text-xl font-semibold">New recording</h1>{actions}</div>
      )}
      <div className="py-3"><AudioSummaryLine changeable={false} live /></div>
      {isRecording && <SilentMicWarning />}
      {failed && (
        <div role="alert" className="flex items-center gap-3 border-b border-border bg-background py-2 text-sm text-foreground">
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
