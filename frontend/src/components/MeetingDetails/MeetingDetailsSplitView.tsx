'use client';

import { useEffect, useState, type CSSProperties, type ReactNode } from 'react';
import { FileText, Sparkles, StickyNote } from 'lucide-react';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { SplitHandle, useSplitRatio } from '@/hooks/useSplitRatio';
import { cn } from '@/lib/utils';
import type { MeetingPageLayout } from '@/lib/meeting-page-layout';

const SUBTAB_STORAGE_KEY = 'meetily.meetingDetails.transcriptPaneRatio';

const TABS = [
  { value: 'transcript' as const, label: 'Transcript', icon: FileText },
  { value: 'notes' as const, label: 'My notes', icon: StickyNote },
  { value: 'summary' as const, label: 'Summary', icon: Sparkles },
];

export type MeetingDetailsTab = 'transcript' | 'notes' | 'summary';

interface MeetingDetailsSplitViewProps {
  transcript: ReactNode;
  notes: ReactNode;
  notesCount: number;
  /** Drives the width of the notes column; the NotesPane inside draws the strip. */
  notesCollapsed?: boolean;
  summary: ReactNode;
  layout: MeetingPageLayout;
  activeTab: MeetingDetailsTab;
  onTabChange: (tab: MeetingDetailsTab) => void;
}

const PANE = 'mt-0 flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden';
const DESKTOP_SHOWN = 'md:data-[state=inactive]:flex';

export function MeetingDetailsSplitView({
  transcript,
  notes,
  notesCount,
  notesCollapsed = false,
  summary,
  layout,
  activeTab,
  onTabChange,
}: MeetingDetailsSplitViewProps) {
  // Subtab layout: transcript | summary.
  const subtab = useSplitRatio({
    storageKey: SUBTAB_STORAGE_KEY,
    defaultRatio: 0.3,
    min: 0.3,
    max: 0.5,
    label: 'transcript and summary',
  });
  // Three columns: transcript | notes | summary. The notes splitter's offset is the transcript width.
  const transcriptCol = useSplitRatio({
    storageKey: 'conclave.threeCol.transcriptRatio',
    defaultRatio: 0.3,
    min: 0.2,
    max: 0.5,
    label: 'transcript',
  });
  const notesCol = useSplitRatio({
    storageKey: 'conclave.threeCol.notesRatio',
    defaultRatio: 0.2,
    min: 0.15,
    max: 0.35,
    offset: transcriptCol.ratio,
    label: 'notes',
  });
  const [isDesktop, setIsDesktop] = useState(true);

  useEffect(() => {
    const mediaQuery = window.matchMedia('(min-width: 768px)');
    const updateLayout = () => setIsDesktop(mediaQuery.matches);
    updateLayout();
    mediaQuery.addEventListener('change', updateLayout);
    return () => mediaQuery.removeEventListener('change', updateLayout);
  }, []);

  // One container for all splitters: each one only moves while its own handle is dragged.
  const setContainer = (el: HTMLDivElement | null) => {
    subtab.containerRef.current = el;
    transcriptCol.containerRef.current = el;
    notesCol.containerRef.current = el;
  };
  const containerHandlers = {
    onPointerMove: (e: React.PointerEvent) => {
      subtab.containerHandlers.onPointerMove(e);
      transcriptCol.containerHandlers.onPointerMove(e);
      notesCol.containerHandlers.onPointerMove(e);
    },
    onPointerUp: () => {
      subtab.containerHandlers.onPointerUp();
      transcriptCol.containerHandlers.onPointerUp();
      notesCol.containerHandlers.onPointerUp();
    },
  };
  const containerProps = {
    ref: setContainer,
    onPointerMove: containerHandlers.onPointerMove,
    onPointerUp: containerHandlers.onPointerUp,
    onPointerCancel: containerHandlers.onPointerUp,
  };

  const panelProps = (label: string) => (isDesktop ? { role: 'region' as const, 'aria-label': label, tabIndex: -1 } : {});

  return (
    <Tabs
      value={activeTab}
      onValueChange={(value) => onTabChange(value as MeetingDetailsTab)}
      className="flex flex-1 min-h-0 min-w-0 flex-col overflow-hidden"
    >
      <div className="shrink-0 bg-background px-2 md:hidden">
        <TabsList className="relative h-auto w-full justify-center rounded-none border-b border-border bg-transparent p-0">
          {TABS.map((tab) => {
            const Icon = tab.icon;
            return (
              <TabsTrigger
                key={tab.value}
                value={tab.value}
                className="relative z-10 flex items-center gap-2 rounded-none border-0 bg-transparent px-6 py-4 text-muted-foreground data-[state=active]:bg-transparent data-[state=active]:text-blue-600 data-[state=active]:shadow-none hover:text-foreground"
              >
                <Icon className="h-4 w-4" />
                {tab.label}
              </TabsTrigger>
            );
          })}
        </TabsList>
      </div>
      {layout === 'subtab' ? (
        <div
          {...containerProps}
          className="flex flex-1 min-h-0 min-w-0 flex-col md:flex-row"
          style={{ '--transcript-pane-width': `${subtab.ratio * 100}%` } as CSSProperties}
        >
          <div className={cn(PANE, 'md:w-[var(--transcript-pane-width)] md:flex-none', activeTab === 'summary' && 'hidden md:flex')}>
            <Tabs
              value={activeTab === 'summary' ? 'transcript' : activeTab}
              onValueChange={(value) => onTabChange(value as MeetingDetailsTab)}
              className="flex min-h-0 min-w-0 flex-1 flex-col"
            >
              <TabsList className="hidden h-9 w-full shrink-0 rounded-none border-b border-border bg-background p-0 md:flex">
                <TabsTrigger value="transcript" className="rounded-none border-0 bg-transparent data-[state=active]:shadow-none">Transcript</TabsTrigger>
                <TabsTrigger value="notes" className="rounded-none border-0 bg-transparent data-[state=active]:shadow-none">My notes ({notesCount})</TabsTrigger>
              </TabsList>
              <TabsContent value="transcript" forceMount className={cn(PANE, 'data-[state=inactive]:hidden')} {...panelProps('Transcript')}>
                {transcript}
              </TabsContent>
              <TabsContent value="notes" forceMount className={cn(PANE, 'data-[state=inactive]:hidden')} {...panelProps('My notes')}>
                {notes}
              </TabsContent>
            </Tabs>
          </div>
          <SplitHandle {...subtab.separatorProps} />
          <TabsContent value="summary" forceMount className={cn(PANE, 'data-[state=inactive]:hidden', DESKTOP_SHOWN)} {...panelProps('Summary')}>
            {summary}
          </TabsContent>
        </div>
      ) : (
        <div
          {...containerProps}
          className="flex flex-1 min-h-0 min-w-0 flex-col md:flex-row"
          style={{
            '--transcript-pane-width': `${transcriptCol.ratio * 100}%`,
            '--notes-pane-width': `${notesCol.ratio * 100}%`,
          } as CSSProperties}
        >
          <TabsContent
            value="transcript"
            forceMount
            className={cn(PANE, 'data-[state=inactive]:hidden md:w-[var(--transcript-pane-width)] md:flex-none', DESKTOP_SHOWN)}
            {...panelProps('Transcript')}
          >
            {transcript}
          </TabsContent>
          <SplitHandle {...transcriptCol.separatorProps} />
          <TabsContent
            value="notes"
            forceMount
            className={cn(PANE, 'data-[state=inactive]:hidden md:flex-none', !notesCollapsed && 'md:w-[var(--notes-pane-width)]', DESKTOP_SHOWN)}
            {...panelProps('My notes')}
          >
            {notes}
          </TabsContent>
          {!notesCollapsed && <SplitHandle {...notesCol.separatorProps} />}
          <TabsContent value="summary" forceMount className={cn(PANE, 'data-[state=inactive]:hidden', DESKTOP_SHOWN)} {...panelProps('Summary')}>
            {summary}
          </TabsContent>
        </div>
      )}
    </Tabs>
  );
}
