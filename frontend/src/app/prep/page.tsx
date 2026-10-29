'use client';

import { Suspense } from 'react';
import { PrepPage } from '@/components/Prep/PrepPage';

export default function Page() {
  return <Suspense fallback={null}><PrepPage /></Suspense>;
}
