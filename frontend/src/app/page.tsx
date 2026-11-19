'use client';

import { StartupChecks } from '@/app/_components/StartupChecks';
import { useCallback, useState } from 'react';
import { useAiSidebar, useAiSidebarScreen } from '@/components/AiSidebar/AiSidebarProvider';
import { PageFrame } from '@/components/PageFrame/PageFrame';
import { MeetingsTable } from '@/components/MeetingsTable';

export default function Home() {
  const { toggle } = useAiSidebar();
  const [view, setView] = useState({ viewName: 'Meetings', meetingIds: [] as string[] });
  useAiSidebarScreen('table', { label: view.viewName || 'Meetings', meetingIds: view.meetingIds });
  const onScopeChange = useCallback((v: { viewName: string; meetingIds: string[] }) => setView(v), []);
  return (
    <PageFrame variant="list" title="Meetings">
      <StartupChecks />
      <MeetingsTable onAsk={toggle} onScopeChange={onScopeChange} />
    </PageFrame>
  );
}
