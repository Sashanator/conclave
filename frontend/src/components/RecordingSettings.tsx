import React, { useState, useEffect } from 'react';
import { Switch } from '@/components/ui/switch';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { FolderOpen } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { DeviceSelection, SelectedDevices } from '@/components/DeviceSelection';
import { toast } from 'sonner';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useConfig } from '@/contexts/ConfigContext';
import { OldAudioReview } from '@/components/OldAudio/OldAudioReview';
import { SettingsRow, useSavedTick } from '@/components/Settings/SettingsRow';
import { NotificationsRow } from '@/components/Settings/NotificationsRow';
import { Button } from '@/components/ui/button';
import { daysToSelect, OLD_AUDIO_OPTIONS, selectToDays } from '@/lib/old-audio';

export interface RecordingPreferences {
  save_folder: string;
  auto_save: boolean;
  file_format: string;
  preferred_mic_device: string | null;
  preferred_system_device: string | null;
  meeting_detection: 'off' | 'ask' | 'auto';
  old_audio_days: number | null;
}

const DETECTION_OPTIONS = [
  { value: 'off', label: 'Off' },
  { value: 'ask', label: 'Ask (show a banner)' },
  { value: 'auto', label: 'Auto-record' },
] as const;

interface RecordingSettingsProps {
  onSave?: (preferences: RecordingPreferences) => void;
}

export function RecordingSettings({ onSave }: RecordingSettingsProps) {
  const [preferences, setPreferences] = useState<RecordingPreferences>({
    save_folder: '',
    auto_save: true,
    file_format: 'mp4',
    preferred_mic_device: null,
    preferred_system_device: null,
    meeting_detection: 'ask',
    old_audio_days: 30,
  });
  const [detectionStatus, setDetectionStatus] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [showRecordingNotification, setShowRecordingNotification] = useState(true);
  const [reviewOpen, setReviewOpen] = useState(false);
  const [saved, markSaved] = useSavedTick();
  const [savedRow, setSavedRow] = useState('');
  const tick = (row: string) => { setSavedRow(row); markSaved(); };
  const { isRecording } = useRecordingState();
  const { setSelectedDevices } = useConfig();

  // Load recording preferences on component mount
  useEffect(() => {
    const loadPreferences = async () => {
      try {
        const prefs = await invoke<RecordingPreferences>('get_recording_preferences');
        setPreferences(prefs);
      } catch (error) {
        console.error('Failed to load recording preferences:', error);
        // If loading fails, get default folder path
        try {
          const defaultPath = await invoke<string>('get_default_recordings_folder_path');
          setPreferences(prev => ({ ...prev, save_folder: defaultPath }));
        } catch (defaultError) {
          console.error('Failed to get default folder path:', defaultError);
        }
      } finally {
        setLoading(false);
      }
    };

    loadPreferences();
  }, []);

  // Null while meeting detection runs; otherwise why it doesn't
  useEffect(() => {
    invoke<string | null>('get_meeting_detection_status')
      .then(setDetectionStatus)
      .catch((error) => console.error('Failed to load meeting detection status:', error));
  }, []);

  // Load recording notification preference
  useEffect(() => {
    const loadNotificationPref = async () => {
      try {
        const { Store } = await import('@tauri-apps/plugin-store');
        const store = await Store.load('preferences.json');
        const show = await store.get<boolean>('show_recording_notification') ?? true;
        setShowRecordingNotification(show);
      } catch (error) {
        console.error('Failed to load notification preference:', error);
      }
    };
    loadNotificationPref();
  }, []);

  const handleAutoSaveToggle = async (enabled: boolean) => {
    const newPreferences = { ...preferences, auto_save: enabled };
    setPreferences(newPreferences);
    await savePreferences(newPreferences, 'Save audio recordings');
  };

  const handleDeviceChange = async (devices: SelectedDevices) => {
    const newPreferences = {
      ...preferences,
      preferred_mic_device: devices.micDevice,
      preferred_system_device: devices.systemDevice
    };
    setPreferences(newPreferences);
    // Sync the in-memory selection ConfigContext exposes to the start path.
    // Without this, the picker only writes to disk (loaded into context once at
    // app mount), so a newly-chosen mic isn't honored until the next launch —
    // start keeps sending the stale launch-time device.
    setSelectedDevices(devices);
    await savePreferences(newPreferences, 'Default audio devices');
  };

  const handleDetectionChange = async (value: string) => {
    const previous = preferences.meeting_detection;
    const meeting_detection = value as RecordingPreferences['meeting_detection'];
    setPreferences({ ...preferences, meeting_detection });
    try {
      await invoke('set_recording_preferences', { preferences: { ...preferences, meeting_detection } });
      tick('Meeting detection');
    } catch (error) {
      console.error('Failed to save meeting detection:', error);
      setPreferences((prev) => ({ ...prev, meeting_detection: previous }));
      toast.error('Failed to save meeting detection', {
        description: error instanceof Error ? error.message : String(error)
      });
    }
  };

  const handleOldAudioChange = async (value: string) => {
    const previous = preferences.old_audio_days;
    const old_audio_days = selectToDays(value);
    setPreferences({ ...preferences, old_audio_days });
    try {
      await invoke('set_recording_preferences', { preferences: { ...preferences, old_audio_days } });
      tick('Remind me about audio older than');
    } catch (error) {
      setPreferences((prev) => ({ ...prev, old_audio_days: previous }));
      toast.error('Failed to save the setting', { description: error instanceof Error ? error.message : String(error) });
    }
  };

  const handleOpenFolder = async () => {
    try {
      await invoke('open_recordings_folder');
    } catch (error) {
      console.error('Failed to open recordings folder:', error);
    }
  };

  const handleNotificationToggle = async (enabled: boolean) => {
    try {
      setShowRecordingNotification(enabled);
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('preferences.json');
      await store.set('show_recording_notification', enabled);
      await store.save();
      tick('Recording start notification');
    } catch (error) {
      console.error('Failed to save notification preference:', error);
      toast.error('Failed to save preference');
    }
  };

  const savePreferences = async (prefs: RecordingPreferences, row: string) => {
    setSaving(true);
    try {
      await invoke('set_recording_preferences', { preferences: prefs });
      onSave?.(prefs);
      tick(row);
    } catch (error) {
      console.error('Failed to save recording preferences:', error);
      toast.error("Failed to save device preferences", {
        description: error instanceof Error ? error.message : String(error)
      });
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return (
      <div className="animate-pulse">
        <div className="h-4 bg-muted rounded w-1/4 mb-4"></div>
        <div className="h-8 bg-muted rounded mb-4"></div>
      </div>
    );
  }

  return (
    <div>
      <SettingsRow label="Save audio recordings" htmlFor="save-audio" saved={saved && savedRow === 'Save audio recordings'} helper="Automatically save audio files when recording stops">
        <Switch id="save-audio" aria-label="Save audio recordings" checked={preferences.auto_save} onCheckedChange={handleAutoSaveToggle} disabled={saving} />
      </SettingsRow>

      {preferences.auto_save && (
        <SettingsRow
          label="Save location"
          helper={`${preferences.save_folder || 'Default folder'} (${preferences.file_format.toUpperCase()}, named recording_YYYYMMDD_HHMMSS.${preferences.file_format})`}
        >
          <Button variant="outline" size="sm" onClick={handleOpenFolder}>
            <FolderOpen className="mr-2 h-4 w-4" />
            Open Folder
          </Button>
        </SettingsRow>
      )}

      <SettingsRow label="Recording start notification" htmlFor="recording-start-notification" saved={saved && savedRow === 'Recording start notification'} helper="Show a reminder to inform participants when recording starts">
        <Switch id="recording-start-notification" aria-label="Recording start notification" checked={showRecordingNotification} onCheckedChange={handleNotificationToggle} />
      </SettingsRow>

      <NotificationsRow />

      <SettingsRow
        label="Meeting detection"
        saved={saved && savedRow === 'Meeting detection'}
        helper={
          <>
            Detects Zoom, Google Meet and Microsoft Teams calls. Recording stops 5 seconds after you leave the call.
            {detectionStatus && <span className="mt-2 block text-amber-700 dark:text-amber-400">{detectionStatus}</span>}
          </>
        }
      >
        <Select value={preferences.meeting_detection} onValueChange={handleDetectionChange}>
          <SelectTrigger className="w-56" aria-label="Meeting detection">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {DETECTION_OPTIONS.map((o) => (
              <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </SettingsRow>

      <SettingsRow
        label="Remind me about audio older than"
        saved={saved && savedRow === 'Remind me about audio older than'}
        helper="Conclave never deletes audio by itself. Transcripts, summaries and notes are always kept. Pin a meeting with “Keep audio” to leave it out."
      >
        <div className="flex flex-col items-start gap-2">
          <Select value={daysToSelect(preferences.old_audio_days)} onValueChange={handleOldAudioChange}>
            <SelectTrigger className="w-56" aria-label="Remind me about audio older than">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {OLD_AUDIO_OPTIONS.map((o) => (
                <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
          <button
            type="button"
            onClick={() => setReviewOpen(true)}
            disabled={preferences.old_audio_days === null || isRecording}
            className="text-sm text-primary hover:underline disabled:opacity-50 disabled:no-underline"
          >
            Review old audio…
          </button>
        </div>
        <OldAudioReview open={reviewOpen} onOpenChange={setReviewOpen} />
      </SettingsRow>

      <SettingsRow
        label="Default audio devices"
        block
        saved={saved && savedRow === 'Default audio devices'}
        helper="Set your preferred microphone and system audio devices for recording. These will be automatically selected when starting new recordings."
      >
        {isRecording && (
          <p
            role="status"
            aria-live="polite"
            className="mb-4 rounded-md border border-amber-200 bg-amber-50 p-2 text-sm text-amber-700 dark:border-amber-800 dark:bg-amber-950/40 dark:text-amber-300"
          >
            Device selection is locked while a recording is in progress. Connecting a new device mid-recording will not switch to it. Stop the current meeting to change devices.
          </p>
        )}
        <DeviceSelection
          selectedDevices={{
            micDevice: preferences.preferred_mic_device,
            systemDevice: preferences.preferred_system_device,
          }}
          onDeviceChange={handleDeviceChange}
          disabled={saving || isRecording}
        />
      </SettingsRow>
    </div>
  );
}
