'use client';
import { useEffect, useState, type ReactNode } from 'react';
import { listen } from '@tauri-apps/api/event';
import { ChevronRight } from 'lucide-react';
import { TranscriptPanel } from './TranscriptPanel';
import { NotesPane } from '@/components/MeetingNotes/NotesPane';
import { CopilotPane } from '@/components/Copilot/CopilotPane';
import { SplitHandle, useSplitRatio } from '@/hooks/useSplitRatio';
import { readFlag, writeFlag } from '@/lib/meeting-page-layout';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { readLiveSideTab, writeLiveSideTab, type LiveSideTab } from '@/contexts/CopilotContext';
import type { ModalType } from '@/hooks/useModalState';
import { annotationsLocked } from '@/lib/meeting-notes';

const tabClass = (on: boolean) =>
  `-mb-px border-b-2 px-2 py-2 text-sm ${on ? 'border-primary font-medium text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground'}`;
const bodyClass = (on: boolean) => (on ? 'flex min-h-0 min-w-0 flex-1 flex-col' : 'hidden');

// children (record bar, status overlays) float over the transcript column, which is the relative anchor below.
export function LiveTranscriptAndNotes({ children, ...props }: { isProcessingStop: boolean; isStopping: boolean; showModal: (name: ModalType, message?: string) => void; children?: ReactNode }) {
  const { notes, addNote, updateNote, deleteNote } = useTranscripts();
  const { isRecording, isSaving, activeDuration } = useRecordingState();
  const locked = annotationsLocked({ isRecording, isStopping: props.isStopping, isProcessing: props.isProcessingStop, isSaving });
  const [collapsed, setCollapsed] = useState(() => readFlag('conclave.notesPaneCollapsed'));
  const [tab, setTab] = useState<LiveSideTab>(readLiveSideTab);
  const split = useSplitRatio({ storageKey: 'conclave.live.transcriptRatio', defaultRatio: 0.7, min: 0.5, max: 0.8, label: 'transcript and notes' });
  const toggle = () => setCollapsed((c) => { writeFlag('conclave.notesPaneCollapsed', !c); return !c; });
  const selectTab = (next: LiveSideTab) => { writeLiveSideTab(next); setTab(next); };
  const notesProps = {
    notes,
    onAdd: (text: string) => addNote(text, activeDuration ?? 0),
    onUpdate: updateNote,
    onDelete: deleteNote,
    onToggleCollapsed: toggle,
    locked,
    disabled: locked || activeDuration === null,
    disabledHint: !isRecording ? 'Notes start with a recording' : locked ? 'Saving the meeting' : 'Starting the recording',
  };

  // The name-alert banner's click: show the Copilot tab, expanded.
  useEffect(() => {
    const unlisten = listen('copilot-open', () => {
      selectTab('copilot');
      setCollapsed(false);
      writeFlag('conclave.notesPaneCollapsed', false);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <div ref={split.containerRef} className="flex flex-1 min-w-0 overflow-hidden" {...split.containerHandlers}>
      <div className="relative flex min-w-0" style={collapsed ? { flex: 1 } : { width: `${split.ratio * 100}%`, flex: 'none' }}>
        <TranscriptPanel {...props} />
        {children}
      </div>
      {!collapsed && <SplitHandle {...split.separatorProps} />}
      {collapsed ? (
        <NotesPane {...notesProps} collapsed />
      ) : (
        <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background">
          <div role="tablist" aria-label="Side panel" className="flex items-center gap-1 border-b border-border px-2">
            <button type="button" role="tab" aria-selected={tab === 'notes'} className={tabClass(tab === 'notes')} onClick={() => selectTab('notes')}>
              {`Notes (${notes.length})`}
            </button>
            <button type="button" role="tab" aria-selected={tab === 'copilot'} className={tabClass(tab === 'copilot')} onClick={() => selectTab('copilot')}>
              Copilot
            </button>
            <button type="button" aria-label="Hide notes" aria-expanded={true} onClick={toggle}
              className="ml-auto rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground">
              <ChevronRight className="h-4 w-4" />
            </button>
          </div>
          {/* Both bodies stay mounted, so a half-typed note or question survives a tab switch. */}
          <div className={bodyClass(tab === 'notes')}>
            <NotesPane {...notesProps} collapsed={false} hideHeader />
          </div>
          <div className={bodyClass(tab === 'copilot')}>
            <CopilotPane locked={locked} />
          </div>
        </div>
      )}
    </div>
  );
}
