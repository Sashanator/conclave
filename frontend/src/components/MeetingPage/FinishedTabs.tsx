'use client';

import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

export type MeetingDetailsTab = 'summary' | 'transcript' | 'notes' | 'prep';

/** A link to a line or a named tab opens it; otherwise the summary. */
export function openingTab(requested: MeetingDetailsTab | null | undefined, focusSegmentId: string | null | undefined): MeetingDetailsTab {
  return requested ?? (focusSegmentId ? 'transcript' : 'summary');
}

const PANE = 'min-h-0 min-w-0 flex-1 flex-col overflow-hidden';

/** The Finished state: four tabs, every pane stays mounted so the editor, the audio and the notes keep their state. */
export function FinishedTabs({ activeTab, onTabChange, notesCount, summary, tasks, transcript, notes, prep }: {
  activeTab: MeetingDetailsTab;
  onTabChange: (tab: MeetingDetailsTab) => void;
  notesCount: number;
  summary: ReactNode;
  tasks: ReactNode;
  transcript: ReactNode;
  notes: ReactNode;
  prep: ReactNode;
}) {
  const tabs: { value: MeetingDetailsTab; label: string }[] = [
    { value: 'summary', label: 'Summary' },
    { value: 'transcript', label: 'Transcript' },
    { value: 'notes', label: `My notes (${notesCount})` },
    { value: 'prep', label: 'Prep' },
  ];
  const pane = (value: MeetingDetailsTab, label: string, body: ReactNode, extra?: string) => (
    <div role="tabpanel" id={`meeting-tab-${value}`} aria-label={label} hidden={activeTab !== value} className={cn(PANE, activeTab === value && 'flex', extra)}>
      {body}
    </div>
  );
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background">
      <div role="tablist" aria-label="Meeting content" className="flex shrink-0 gap-1 border-b border-border px-3">
        {tabs.map((t) => (
          <button
            key={t.value}
            type="button"
            role="tab"
            aria-selected={activeTab === t.value}
            aria-controls={`meeting-tab-${t.value}`}
            onClick={() => onTabChange(t.value)}
            className={cn('-mb-px border-b-2 px-3 py-2 text-sm', activeTab === t.value ? 'border-primary font-medium text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground')}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div hidden={activeTab !== 'summary'} role="tabpanel" id="meeting-tab-summary" aria-label="Summary" className={cn('min-h-0 min-w-0 flex-1 overflow-hidden', activeTab === 'summary' && 'flex flex-col md:flex-row')}>
        <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">{summary}</div>
        <div className="shrink-0 overflow-y-auto border-t border-border md:w-80 md:border-l md:border-t-0">{tasks}</div>
      </div>
      {pane('transcript', 'Transcript', transcript)}
      {pane('notes', 'My notes', notes)}
      {pane('prep', 'Prep', prep)}
    </div>
  );
}
