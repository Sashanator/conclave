'use client';

import { useEffect, useState } from 'react';
import { ArrowLeft } from 'lucide-react';
import { useRouter } from 'next/navigation';
import { goBack, originIsPrevious, readOrigin } from '@/lib/back-nav';
import { useBackShortcut } from '@/hooks/useBackShortcut';

export function BackCrumb({ fallback, fallbackLabel }: { fallback: string; fallbackLabel: string }) {
  const router = useRouter();
  const [label, setLabel] = useState(fallbackLabel);
  // Read after mount: sessionStorage does not exist in the static export.
  useEffect(() => { setLabel(originIsPrevious() ? readOrigin()!.label : fallbackLabel); }, [fallbackLabel]);
  const back = () => void goBack(router, fallback);
  useBackShortcut(back);
  return (
    <button type="button" aria-label={`Back to ${label}`} onClick={back} className="inline-flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground">
      <ArrowLeft className="h-4 w-4" aria-hidden />
      {label}
      <kbd className="ml-1 rounded border border-border px-1 text-[11px]">Esc</kbd>
    </button>
  );
}
