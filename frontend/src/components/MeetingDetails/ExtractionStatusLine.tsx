'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/button';

// Pending is re-checked on a timer, since the extraction worker emits no event
const PENDING_RECHECK_MS = 3000;

/** Task extraction after a summary. refreshKey (the summary status) rechecks it. */
export function ExtractionStatusLine({ meetingId, refreshKey }: { meetingId: string; refreshKey?: string }) {
  const [status, setStatus] = useState<string | null>(null);
  const [retrying, setRetrying] = useState(false);

  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const check = () => {
      invoke<string | null>('extraction_status', { meetingId })
        .then((s) => {
          if (!live) return;
          setStatus(s);
          if (s === 'pending') timer = setTimeout(check, PENDING_RECHECK_MS);
        })
        .catch((err) => console.error('Extraction status failed to load:', err));
    };
    check();
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [meetingId, refreshKey]);

  const retry = async () => {
    setRetrying(true);
    setStatus('pending');
    try {
      await invoke('extraction_retry', { meetingId });
    } catch (err) {
      console.error('Extraction retry failed:', err);
    }
    setRetrying(false);
    setStatus(await invoke<string | null>('extraction_status', { meetingId }).catch(() => 'failed'));
  };

  if (status === 'pending') return <p className="mt-2 text-sm text-muted-foreground">Extracting tasks…</p>;
  if (status !== 'failed') return null;

  return (
    <div className="mt-2 flex items-center gap-2 text-sm">
      <span className="text-red-600">Task extraction failed</span>
      <Button variant="outline" size="sm" onClick={retry} disabled={retrying}>
        Retry
      </Button>
    </div>
  );
}
