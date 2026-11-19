'use client';

import { useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Switch } from '@/components/ui/switch';
import { SettingsRow, useSavedTick } from '@/components/Settings/SettingsRow';
import { useConfig } from '@/contexts/ConfigContext';

const LABEL = 'Windows notifications when recording starts/stops';

export function NotificationsRow() {
  const { notificationSettings, loadPreferences, updateNotificationSettings } = useConfig();
  const [saved, markSaved] = useSavedTick();
  // Shown at once; goes back to the stored value when the save fails.
  const [optimistic, setOptimistic] = useState<boolean | null>(null);
  useEffect(() => {
    loadPreferences();
  }, [loadPreferences]);

  const p = notificationSettings?.notification_preferences;
  // Nothing stored means on, as before.
  const stored = p ? p.show_recording_started && p.show_recording_stopped : true;

  const change = async (on: boolean) => {
    if (!notificationSettings) return;
    setOptimistic(on);
    try {
      await updateNotificationSettings({
        ...notificationSettings,
        notification_preferences: { ...notificationSettings.notification_preferences, show_recording_started: on, show_recording_stopped: on },
      });
      markSaved();
    } catch (err) {
      toast.error('Failed to save the notification setting', { description: String(err) });
    } finally {
      setOptimistic(null);
    }
  };

  return (
    <SettingsRow label={LABEL} htmlFor="windows-notifications" saved={saved} helper="Windows notifications when a recording starts or stops">
      <Switch id="windows-notifications" aria-label={LABEL} checked={optimistic ?? stored} disabled={!notificationSettings} onCheckedChange={change} />
    </SettingsRow>
  );
}
