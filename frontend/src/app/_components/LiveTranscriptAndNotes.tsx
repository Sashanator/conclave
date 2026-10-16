'use client';
import { useState } from 'react';
import { TranscriptPanel } from './TranscriptPanel';
import { NotesPane } from '@/components/MeetingNotes/NotesPane';
import { SplitHandle, useSplitRatio } from '@/hooks/useSplitRatio';
import { readFlag, writeFlag } from '@/lib/meeting-page-layout';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import type { ModalType } from '@/hooks/useModalState';

export function LiveTranscriptAndNotes(props: { isProcessingStop: boolean; isStopping: boolean; showModal: (name: ModalType, message?: string) => void }) {
  const { notes, addNote, updateNote, deleteNote } = useTranscripts();
  const { isRecording, activeDuration } = useRecordingState();
  const [collapsed, setCollapsed] = useState(() => readFlag('conclave.notesPaneCollapsed'));
  const split = useSplitRatio({ storageKey: 'conclave.live.transcriptRatio', defaultRatio: 0.7, min: 0.5, max: 0.8, label: 'transcript and notes' });
  const toggle = () => setCollapsed((c) => { writeFlag('conclave.notesPaneCollapsed', !c); return !c; });

  return (
    <div ref={split.containerRef} className="flex flex-1 min-w-0 overflow-hidden" {...split.containerHandlers}>
      <div className="flex min-w-0" style={collapsed ? { flex: 1 } : { width: `${split.ratio * 100}%`, flex: 'none' }}>
        <TranscriptPanel {...props} />
      </div>
      {!collapsed && <SplitHandle {...split.separatorProps} />}
      <NotesPane
        notes={notes}
        onAdd={(text) => addNote(text, activeDuration ?? 0)}
        onUpdate={updateNote}
        onDelete={deleteNote}
        collapsed={collapsed}
        onToggleCollapsed={toggle}
        disabled={!isRecording}
        disabledHint="Notes start with a recording"
      />
    </div>
  );
}
