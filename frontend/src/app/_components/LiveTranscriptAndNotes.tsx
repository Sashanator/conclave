'use client';
import { ContinueControl } from './ContinueControl';
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
import { toast } from 'sonner';
import { getPrep, prepCounter, type MeetingPrep } from '@/lib/prep';
import { PrepTickList } from '@/components/Prep/PrepTickList';

const tabClass = (on: boolean) =>
  `-mb-px border-b-2 px-2 py-2 text-sm ${on ? 'border-primary font-medium text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground'}`;
const bodyClass = (on: boolean) => (on ? 'flex min-h-0 min-w-0 flex-1 flex-col' : 'hidden');

// children (record bar, status overlays) float over the transcript column, which is the relative anchor below.
export function LiveTranscriptAndNotes({ children, ...props }: { isProcessingStop: boolean; isStopping: boolean; showModal: (name: ModalType, message?: string) => void; children?: ReactNode }) {
  const { notes, addNote, updateNote, deleteNote, calendarLink } = useTranscripts();
  const { isRecording, isSaving, activeDuration } = useRecordingState();
  const locked = annotationsLocked({ isRecording, isStopping: props.isStopping, isProcessing: props.isProcessingStop, isSaving });
  const [collapsed, setCollapsed] = useState(() => readFlag('conclave.notesPaneCollapsed'));
  const [tab, setTab] = useState<LiveSideTab>(readLiveSideTab);
  const [prep, setPrep] = useState<MeetingPrep | null>(null);
  const shownTab = tab === 'prep' && !prep ? 'notes' : tab;
  const uid = calendarLink?.uid;
  const start = calendarLink?.start;

  useEffect(() => {
    let active = true;
    setPrep(null);
    if (isRecording && uid && start) {
      getPrep(uid, start)
        .then((value) => { if (active) setPrep(value); })
        .catch((error) => { if (active) toast.error(`Failed to load Prep: ${String(error)}`); });
    }
    return () => { active = false; };
  }, [isRecording, uid, start]);
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
        {isRecording && <ContinueControl />}
        <TranscriptPanel {...props} />
        {children}
      </div>
      {!collapsed && <SplitHandle {...split.separatorProps} />}
      {collapsed ? (
        <NotesPane {...notesProps} collapsed />
      ) : (
        <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background">
          <div role="tablist" aria-label="Side panel" className="flex items-center gap-1 border-b border-border px-2">
            <button type="button" role="tab" aria-selected={shownTab === 'notes'} className={tabClass(shownTab === 'notes')} onClick={() => selectTab('notes')}>
              {`Notes (${notes.length})`}
            </button>
            <button type="button" role="tab" aria-selected={shownTab === 'copilot'} className={tabClass(shownTab === 'copilot')} onClick={() => selectTab('copilot')}>
              Copilot
            </button>
            {prep && <button type="button" role="tab" aria-selected={shownTab === 'prep'} className={tabClass(shownTab === 'prep')} onClick={() => selectTab('prep')}>
              {prepCounter(prep.items)}
            </button>}
            <button type="button" aria-label="Hide notes" aria-expanded={true} onClick={toggle}
              className="ml-auto rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground">
              <ChevronRight className="h-4 w-4" />
            </button>
          </div>
          {/* All bodies stay mounted, so a half-typed note or question survives a tab switch. */}
          <div className={bodyClass(shownTab === 'notes')}>
            <NotesPane {...notesProps} collapsed={false} hideHeader />
          </div>
          <div className={bodyClass(shownTab === 'copilot')}>
            <CopilotPane locked={locked} />
          </div>
          <div className={bodyClass(shownTab === 'prep')}>
            {prep && <PrepTickList prep={prep} onChange={setPrep} />}
          </div>
        </div>
      )}
    </div>
  );
}
