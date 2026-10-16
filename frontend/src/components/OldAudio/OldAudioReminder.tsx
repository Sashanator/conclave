'use client';
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { OldAudioReview } from './OldAudioReview';
import { formatMegabytes, recordingCount, REMINDER_DAY_KEY, shouldRemind, todayKey, type OldAudio } from '@/lib/old-audio';

const DAY_MS = 24 * 60 * 60 * 1000;

export function OldAudioReminder() {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const check = async () => {
      let last: string | null = null;
      try { last = localStorage.getItem(REMINDER_DAY_KEY); } catch { /* reminder still works */ }
      let items: OldAudio[] = [];
      try { items = await invoke<OldAudio[]>('old_audio_list'); } catch { return; }
      const today = todayKey();
      if (!shouldRemind(last, today, items.length)) return;
      try { localStorage.setItem(REMINDER_DAY_KEY, today); } catch { /* optional */ }
      const bytes = items.reduce((s, i) => s + i.bytes, 0);
      let days: number | null = null;
      try { days = (await invoke<{ old_audio_days: number | null }>('get_recording_preferences')).old_audio_days; } catch { /* generic text */ }
      const age = days ? `older than ${days} days` : 'old';
      toast(`${recordingCount(items.length)} ${items.length === 1 ? 'is' : 'are'} ${age} (${formatMegabytes(bytes)}).`, {
        description: 'Nothing is deleted until you review and confirm.',
        action: { label: 'Review', onClick: () => setOpen(true) },
        duration: 15000,
      });
    };
    void check();
    const id = setInterval(check, DAY_MS);
    return () => clearInterval(id);
  }, []);

  return <OldAudioReview open={open} onOpenChange={setOpen} />;
}
