'use client';

import { TaskList } from '@/components/Tasks/TaskList';

export default function TasksPage() {
  return (
    <div className="h-full overflow-y-auto bg-muted/50">
      <div className="mx-auto max-w-3xl space-y-6 p-8">
        <h1 className="text-3xl font-bold">Tasks</h1>
        <TaskList />
      </div>
    </div>
  );
}
