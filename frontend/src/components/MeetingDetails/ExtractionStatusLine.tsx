'use client';

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/button';

// Pending is re-checked on a timer, since the extraction worker emits no event
const PENDING_RECHECK_MS = 3000;

/** Task extraction after a summary. refreshKey (the summary status) rechecks it. onSettled runs when a pending extraction finishes. */
export function ExtractionStatusLine({
  meetingId,
  refreshKey,
  recheckMs = PENDING_RECHECK_MS,
  onSettled,
}: {
  meetingId: string;
  refreshKey?: string;
  recheckMs?: number;
  onSettled?: () => void;
}) {
  const [status, setStatus] = useState<string | null>(null);
  const [retrying, setRetrying] = useState(false);
  // Bumped after a retry, so the check runs again even when the status was already pending
  const [checkKey, setCheckKey] = useState(0);
  // A ref, so a recheck that starts while pending still reports the finish
  const wasPending = useRef(false);

  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const check = () => {
      invoke<string | null>('extraction_status', { meetingId })
        .then((s) => {
          if (!live) return;
          setStatus(s);
          if (s === 'pending') {
            wasPending.current = true;
            timer = setTimeout(check, recheckMs);
          } else if (wasPending.current) {
            wasPending.current = false;
            onSettled?.();
          }
        })
        .catch((err) => console.error('Extraction status failed to load:', err));
    };
    check();
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [meetingId, refreshKey, checkKey, recheckMs]);

  const retry = async () => {
    setRetrying(true);
    setStatus('pending');
    try {
      await invoke('extraction_retry', { meetingId });
    } catch (err) {
      console.error('Extraction retry failed:', err);
    }
    setRetrying(false);
    setCheckKey((k) => k + 1);
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
