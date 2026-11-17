'use client';

import { useEffect, useRef, type ReactNode } from 'react';
import { useRouter } from 'next/navigation';
import { listen } from '@tauri-apps/api/event';
import { writeSidebarOpen } from '@/lib/ai-sidebar';
import { CopilotPane } from '@/components/Copilot/CopilotPane';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { annotationsLocked } from '@/lib/meeting-notes';
import { useAiSidebar } from './AiSidebarProvider';

function LiveBody() {
  return <CopilotPane locked={annotationsLocked(useRecordingState())} />;
}

/** Right-hand AI panel shell; Ask and Copilot render into `children`. */
export function AiSidebar({ children }: { children?: ReactNode }) {
  const { kind, open } = useAiSidebar();
  if (!kind || !open) return null;
  return (
    <aside aria-label="AI sidebar" className="flex h-screen w-[360px] shrink-0 flex-col border-l border-border bg-background">
      {kind === 'live' ? <LiveBody /> : children}
    </aside>
  );
}

/** The name-alert banner's click: open the live meeting with the sidebar showing. Never window.location: that would reload over a live recording. */
export function CopilotOpenListener() {
  const router = useRouter();
  const { kind, open, toggle } = useAiSidebar();
  // liveMeetingId comes from Task 13 (recording-target); until it lands the field is absent and we only open the sidebar.
  const { liveMeetingId } = useRecordingState() as { liveMeetingId?: string | null };
  const ref = useRef({ kind, open, toggle, liveMeetingId });
  ref.current = { kind, open, toggle, liveMeetingId };
  useEffect(() => {
    const un = listen('copilot-open', () => {
      const r = ref.current;
      if (r.kind === 'live') { if (!r.open) r.toggle(); return; }
      writeSidebarOpen('live', true);
      if (r.liveMeetingId) router.push(`/meeting-details?id=${r.liveMeetingId}`);
    });
    return () => { un.then((fn) => fn()); };
  }, [router]);
  return null;
}
