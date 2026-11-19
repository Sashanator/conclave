'use client';

import type { ReactNode } from 'react';
import { AskChat } from '@/components/Ask/AskChat';
import { useAiSidebar } from '@/components/AiSidebar/AiSidebarProvider';
import { cn } from '@/lib/utils';

/** The meeting page's right column: Tasks | Ask. Ctrl J flips the tab through the AI sidebar state. */
export function MeetingRail({ tasks, taskCount }: { tasks: ReactNode; taskCount: number }) {
  const { open, scope, toggle, close } = useAiSidebar();
  const tab = (selected: boolean, id: string, label: string, onClick: () => void) => (
    <button
      type="button"
      role="tab"
      id={`rail-tab-${id}`}
      aria-selected={selected}
      aria-controls={`rail-panel-${id}`}
      onClick={onClick}
      className={cn('-mb-px border-b-2 px-3 py-2 text-sm', selected ? 'border-primary font-medium text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground')}
    >
      {label}
    </button>
  );
  return (
    <aside aria-label="Tasks and Ask" className="flex min-h-0 shrink-0 flex-col border-t border-border md:w-80 md:border-l md:border-t-0">
      <div role="tablist" aria-label="Tasks and Ask" className="flex shrink-0 gap-1 border-b border-border px-3">
        {tab(!open, 'tasks', `Tasks (${taskCount})`, () => open && close())}
        {tab(open, 'ask', 'Ask', () => !open && toggle())}
      </div>
      <div role="tabpanel" id="rail-panel-tasks" aria-labelledby="rail-tab-tasks" hidden={open} className="min-h-0 flex-1 overflow-y-auto">{tasks}</div>
      <div role="tabpanel" id="rail-panel-ask" aria-labelledby="rail-tab-ask" hidden={!open} className={cn('min-h-0 flex-1 flex-col overflow-y-auto', open && 'flex')}>
        {scope && <AskChat key={scope.meetingIds?.[0]} scope={scope} allowAll={false} />}
      </div>
    </aside>
  );
}
