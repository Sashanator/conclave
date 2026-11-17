'use client';

import { StartupChecks } from '@/app/_components/StartupChecks';
import { MeetingsTable } from '@/components/MeetingsTable';

export default function Home() {
  return (
    // A bounded column: the table's scroll area takes the rest, so long lists are virtualized.
    <div className="flex h-full min-w-0 flex-col py-4 pr-6">
      <StartupChecks />
      <h1 className="mb-3 shrink-0 text-xl font-semibold">Meetings</h1>
      <MeetingsTable />
    </div>
  );
}
