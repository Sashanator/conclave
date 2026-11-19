'use client';

import { useEffect } from 'react';
import { toast } from 'sonner';
import { hasAiKeyHint, clearAiKeyHint } from '@/lib/ai-key-hint';
import { settingsHref } from '@/lib/settings-panes';

/** Shown once after onboarding, in the app shell: the reload at the end of onboarding wipes toasts raised during it. */
export function AiKeyHint() {
  useEffect(() => {
    // Deferred: the Toaster is a later sibling and must have subscribed first (sonner does not replay).
    const t = setTimeout(() => {
      if (!hasAiKeyHint()) return;
      toast.error('Could not save your Claude key', {
        description: 'Add it in Settings → Models & keys.',
        duration: 15000,
        action: { label: 'Open Settings', onClick: () => window.location.assign(settingsHref('models')) },
      });
      clearAiKeyHint(); // only after the toast call
    }, 0);
    return () => clearTimeout(t);
  }, []);
  return null;
}
