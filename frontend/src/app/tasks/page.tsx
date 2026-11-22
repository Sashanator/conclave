'use client';

import { PageFrame } from '@/components/PageFrame/PageFrame';
import { TasksQueue } from '@/components/Tasks/TasksQueue';

export default function TasksPage() {
  return (
    <PageFrame variant="list" title="Tasks">
      <TasksQueue />
    </PageFrame>
  );
}
