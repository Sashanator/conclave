'use client';

import { StartupChecks } from '@/app/_components/StartupChecks';
import { MeetingsTable } from '@/components/MeetingsTable';

export default function Home() {
  return (
    <main className="px-6 py-4">
      <StartupChecks />
      <h1 className="mb-3 text-xl font-semibold">Meetings</h1>
      <MeetingsTable />
    </main>
  );
}
