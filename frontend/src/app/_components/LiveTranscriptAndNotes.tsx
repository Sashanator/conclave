'use client';
import { ContinueControl } from './ContinueControl';
import { useEffect, useState, type ReactNode } from 'react';
import { TranscriptPanel } from './TranscriptPanel';
import { NotesPane } from '@/components/MeetingNotes/NotesPane';
import { SplitHandle, useSplitRatio } from '@/hooks/useSplitRatio';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import type { ModalType } from '@/hooks/useModalState';
import { annotationsLocked } from '@/lib/meeting-notes';
import { toast } from 'sonner';
import { getPrep, prepForMeeting, type MeetingPrep } from '@/lib/prep';
import { PrepTickList } from '@/components/Prep/PrepTickList';

const tabClass = (on: boolean) =>
  `-mb-px border-b-2 px-2 py-2 text-sm ${on ? 'border-primary font-medium text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground'}`;
const bodyClass = (on: boolean) => (on ? 'flex min-h-0 min-w-0 flex-1 flex-col' : 'hidden');

// children (record bar, status overlays) float over the transcript column, which is the relative anchor below.
// tabbed: My notes is a tab beside Transcript (the AI sidebar is open); otherwise two columns.
// meetingId: the meeting whose prep points show under My notes; without it the calendar link's prep is used.
export function LiveTranscriptAndNotes({ children, tabbed = false, meetingId, ...props }: { isProcessingStop: boolean; isStopping: boolean; showModal: (name: ModalType, message?: string) => void; children?: ReactNode; tabbed?: boolean; meetingId?: string }) {
  const { notes, addNote, updateNote, deleteNote, calendarLink } = useTranscripts();
  const { isRecording, isSaving, activeDuration } = useRecordingState();
  const locked = annotationsLocked({ isRecording, isStopping: props.isStopping, isProcessing: props.isProcessingStop, isSaving });
  const [tab, setTab] = useState<'transcript' | 'notes'>('transcript');
  const [prep, setPrep] = useState<MeetingPrep | null>(null);
  const uid = calendarLink?.uid;
  const start = calendarLink?.start;

  useEffect(() => {
    let active = true;
    setPrep(null);
    const load = meetingId ? prepForMeeting(meetingId) : isRecording && uid && start ? getPrep(uid, start) : null;
    load?.then((value) => { if (active) setPrep(value); })
      .catch((error) => { if (active) toast.error(`Failed to load Prep: ${String(error)}`); });
    return () => { active = false; };
  }, [meetingId, isRecording, uid, start]);
  const split = useSplitRatio({ storageKey: 'conclave.live.transcriptRatio', defaultRatio: 0.6, min: 0.4, max: 0.8, label: 'transcript and notes' });
  const notesPane = (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background">
      {!tabbed && <h2 className="border-b border-border px-3 py-2 text-sm font-medium text-foreground">My notes</h2>}
      <NotesPane
        notes={notes}
        onAdd={(text) => addNote(text, activeDuration ?? 0)}
        onUpdate={updateNote}
        onDelete={deleteNote}
        onToggleCollapsed={() => {}}
        collapsed={false}
        hideHeader
        locked={locked}
        disabled={locked || activeDuration === null}
        disabledHint={!isRecording ? 'Notes start with a recording' : locked ? 'Saving the meeting' : 'Starting the recording'}
      />
      {prep && prep.items.length > 0 && <PrepTickList prep={prep} onChange={setPrep} />}
    </div>
  );
  const transcript = (
    <div className="relative flex min-h-0 min-w-0 flex-1">
      {isRecording && <ContinueControl />}
      <TranscriptPanel {...props} />
      {children}
    </div>
  );

  if (tabbed) {
    return (
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        <div role="tablist" aria-label="Live panels" className="flex items-center gap-1 border-b border-border px-2">
          <button type="button" role="tab" aria-selected={tab === 'transcript'} className={tabClass(tab === 'transcript')} onClick={() => setTab('transcript')}>Transcript</button>
          <button type="button" role="tab" aria-selected={tab === 'notes'} className={tabClass(tab === 'notes')} onClick={() => setTab('notes')}>My notes</button>
        </div>
        {/* Both bodies stay mounted, so a half-typed note survives a tab switch. */}
        <div className={bodyClass(tab === 'transcript')}>{transcript}</div>
        <div className={bodyClass(tab === 'notes')}>{notesPane}</div>
      </div>
    );
  }
  return (
    <div ref={split.containerRef} className="flex flex-1 min-w-0 overflow-hidden" {...split.containerHandlers}>
      <div className="flex min-w-0" style={{ width: `${split.ratio * 100}%`, flex: 'none' }}>{transcript}</div>
      <SplitHandle {...split.separatorProps} />
      {notesPane}
    </div>
  );
}
