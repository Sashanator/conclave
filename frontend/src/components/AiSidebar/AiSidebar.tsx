'use client';

import type { ReactNode } from 'react';
import { useAiSidebar } from './AiSidebarProvider';

/** Right-hand AI panel shell; Ask and Copilot render into `children`. */
export function AiSidebar({ children }: { children?: ReactNode }) {
  const { kind, open } = useAiSidebar();
  if (!kind || !open) return null;
  return (
    <aside aria-label="AI sidebar" className="flex h-screen w-[360px] shrink-0 flex-col border-l border-border bg-background">
      {children}
    </aside>
  );
}
