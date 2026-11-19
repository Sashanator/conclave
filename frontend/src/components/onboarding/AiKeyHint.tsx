'use client';

import { useEffect } from 'react';
import { toast } from 'sonner';
import { takeAiKeyHint } from '@/lib/ai-key-hint';
import { settingsHref } from '@/lib/settings-panes';

/** Shown once after onboarding, in the app shell: the reload at the end of onboarding wipes toasts raised during it. */
export function AiKeyHint() {
  useEffect(() => {
    if (!takeAiKeyHint()) return;
    toast.error('Could not save your Claude key', {
      description: 'Add it in Settings → Models & keys.',
      duration: 15000,
      action: { label: 'Open Settings', onClick: () => window.location.assign(settingsHref('models')) },
    });
  }, []);
  return null;
}
