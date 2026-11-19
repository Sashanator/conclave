'use client';

import { PageFrame } from '@/components/PageFrame/PageFrame';
import { TaskList } from '@/components/Tasks/TaskList';

export default function TasksPage() {
  return (
    <PageFrame variant="list" title="Tasks">
      {/* the list frame is height-bounded, so the list scrolls itself */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        <TaskList />
      </div>
    </PageFrame>
  );
}
