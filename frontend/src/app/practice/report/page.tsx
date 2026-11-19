'use client';

import { Suspense } from 'react';
import { useSearchParams } from 'next/navigation';
import { ReportView } from '@/components/Practice/ReportView';

function ReportRoute() {
  const id = useSearchParams().get('id');
  if (!id) return <p className="p-8 text-muted-foreground">No session selected</p>;
  return <ReportView key={id} sessionId={id} />;
}

export default function PracticeReportPage() {
  return (
    <Suspense fallback={null}>
      <ReportRoute />
    </Suspense>
  );
}
