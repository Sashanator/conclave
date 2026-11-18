'use client';

import { StartupChecks } from '@/app/_components/StartupChecks';
import { useCallback, useState } from 'react';
import { useAiSidebar, useAiSidebarScreen } from '@/components/AiSidebar/AiSidebarProvider';
import { MeetingsTable } from '@/components/MeetingsTable';

export default function Home() {
  const { toggle } = useAiSidebar();
  const [view, setView] = useState({ viewName: 'Meetings', meetingIds: [] as string[] });
  useAiSidebarScreen('table', { label: view.viewName || 'Meetings', meetingIds: view.meetingIds });
  const onScopeChange = useCallback((v: { viewName: string; meetingIds: string[] }) => setView(v), []);
  return (
    // A bounded column: the table's scroll area takes the rest, so long lists are virtualized.
    <div className="flex h-full min-w-0 flex-col py-4 pr-6">
      <StartupChecks />
      <h1 className="mb-3 shrink-0 text-xl font-semibold">Meetings</h1>
      <MeetingsTable onAsk={toggle} onScopeChange={onScopeChange} />
    </div>
  );
}
