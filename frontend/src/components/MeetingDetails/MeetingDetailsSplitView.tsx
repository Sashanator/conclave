'use client';

import { useEffect, useState, type CSSProperties, type ReactNode } from 'react';
import { FileText, Sparkles } from 'lucide-react';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { SplitHandle, useSplitRatio } from '@/hooks/useSplitRatio';

const STORAGE_KEY = 'meetily.meetingDetails.transcriptPaneRatio';
const DEFAULT_RATIO = 0.3;
const MIN_RATIO = 0.3;
const MAX_RATIO = 0.5;

const TABS = [
  { value: 'transcript' as const, label: 'Transcript', icon: FileText },
  { value: 'summary' as const, label: 'Summary', icon: Sparkles },
];

export type MeetingDetailsTab = 'transcript' | 'summary';

interface MeetingDetailsSplitViewProps {
  transcript: ReactNode;
  summary: ReactNode;
  activeTab: MeetingDetailsTab;
  onTabChange: (tab: MeetingDetailsTab) => void;
}

export function MeetingDetailsSplitView({
  transcript,
  summary,
  activeTab,
  onTabChange,
}: MeetingDetailsSplitViewProps) {
  const { ratio, containerRef, containerHandlers, separatorProps } = useSplitRatio({
    storageKey: STORAGE_KEY,
    defaultRatio: DEFAULT_RATIO,
    min: MIN_RATIO,
    max: MAX_RATIO,
    label: 'transcript and summary',
  });
  const [isDesktop, setIsDesktop] = useState(true);

  useEffect(() => {
    const mediaQuery = window.matchMedia('(min-width: 768px)');
    const updateLayout = () => setIsDesktop(mediaQuery.matches);
    updateLayout();
    mediaQuery.addEventListener('change', updateLayout);
    return () => mediaQuery.removeEventListener('change', updateLayout);
  }, []);

  const transcriptPanelProps = isDesktop
    ? { role: 'region' as const, 'aria-label': 'Transcript', tabIndex: -1 }
    : {};
  const summaryPanelProps = isDesktop
    ? { role: 'region' as const, 'aria-label': 'Summary', tabIndex: -1 }
    : {};

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
      <div
        ref={containerRef}
        className="flex flex-1 min-h-0 min-w-0 flex-col md:flex-row"
        style={{ '--transcript-pane-width': `${ratio * 100}%` } as CSSProperties}
        {...containerHandlers}
      >
        <TabsContent
          value="transcript"
          forceMount
          className="mt-0 flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden data-[state=inactive]:hidden md:w-[var(--transcript-pane-width)] md:flex-none md:data-[state=inactive]:flex"
          {...transcriptPanelProps}
        >
          {transcript}
        </TabsContent>
        <SplitHandle {...separatorProps} />
        <TabsContent
          value="summary"
          forceMount
          className="mt-0 flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden data-[state=inactive]:hidden md:data-[state=inactive]:flex"
          {...summaryPanelProps}
        >
          {summary}
        </TabsContent>
      </div>
    </Tabs>
  );
}
