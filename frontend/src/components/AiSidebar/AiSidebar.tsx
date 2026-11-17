'use client';

import { useEffect, useRef, type ReactNode } from 'react';
import { useRouter } from 'next/navigation';
import { listen } from '@tauri-apps/api/event';
import { writeSidebarOpen } from '@/lib/ai-sidebar';
import { CopilotPane } from '@/components/Copilot/CopilotPane';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { AskChat } from '@/components/Ask/AskChat';
import { annotationsLocked } from '@/lib/meeting-notes';
import { useAiSidebar } from './AiSidebarProvider';

function LiveBody() {
  return <CopilotPane locked={annotationsLocked(useRecordingState())} />;
}

/** Right-hand AI panel shell; Ask and Copilot render into `children`. */
export function AiSidebar({ children }: { children?: ReactNode }) {
  const { kind, open, scope } = useAiSidebar();
  if (!kind || !open) return null;
  return (
    <aside aria-label="AI sidebar" className="flex h-screen w-[360px] shrink-0 flex-col border-l border-border bg-background">
      {kind === 'live' ? <LiveBody /> : scope ? <AskChat key={kind === 'meeting' ? scope.meetingIds?.[0] : kind} scope={scope} allowAll={kind === 'table'} /> : children}
    </aside>
  );
}

/** The name-alert banner's click: open the live meeting with the sidebar showing. Never window.location: that would reload over a live recording. */
export function CopilotOpenListener() {
  const router = useRouter();
  const { kind, open, toggle } = useAiSidebar();
  const { liveMeetingId, isRecording } = useRecordingState();
  const ref = useRef({ kind, open, toggle, liveMeetingId, isRecording });
  ref.current = { kind, open, toggle, liveMeetingId, isRecording };
  useEffect(() => {
    const un = listen('copilot-open', () => {
      const r = ref.current;
      if (r.kind === 'live') { if (!r.open) r.toggle(); return; }
      writeSidebarOpen('live', true);
      if (r.liveMeetingId) router.push(`/meeting-details?id=${r.liveMeetingId}`);
      else if (r.isRecording) router.push('/meeting-details'); // a recording with no meeting row
    });
    return () => { un.then((fn) => fn()); };
  }, [router]);
  return null;
}
