"use client"

import { useEffect } from "react"
import { FolderOpen } from "lucide-react"
import { invoke } from "@tauri-apps/api/core"
import { useConfig } from "@/contexts/ConfigContext"
import { useTheme } from "@/contexts/ThemeContext"
import { parseThemePreference } from "@/lib/theme"
import { Button } from "@/components/ui/button"
import { SettingsRow } from "@/components/Settings/SettingsRow"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"

export function PreferenceSettings() {
  const {
    storageLocations,
    isLoadingPreferences,
    loadPreferences
  } = useConfig();
  const { preference, setPreference } = useTheme();

  // Lazy load preferences on mount (only loads if not already cached)
  useEffect(() => {
    loadPreferences();
  }, [loadPreferences]);

  const handleOpenFolder = async (folderType: 'database' | 'models' | 'recordings') => {
    try {
      switch (folderType) {
        case 'database':
          await invoke('open_database_folder');
          break;
        case 'models':
          await invoke('open_models_folder');
          break;
        case 'recordings':
          await invoke('open_recordings_folder');
          break;
      }
    } catch (error) {
      console.error(`Failed to open ${folderType} folder:`, error);
    }
  };

  // Show loading only if we're actually loading and don't have cached data
  if (isLoadingPreferences && !storageLocations) {
    return <div className="p-6">Loading Preferences...</div>
  }

  return (
    <div>
      <SettingsRow label="Theme" helper="Theme for the app and the meeting banner">
        <Select value={preference} onValueChange={(v) => setPreference(parseThemePreference(v))}>
          <SelectTrigger className="w-56" aria-label="Theme">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="system">System (follows Windows)</SelectItem>
            <SelectItem value="light">Light</SelectItem>
            <SelectItem value="dark">Dark</SelectItem>
            <SelectItem value="conclave">Conclave</SelectItem>
          </SelectContent>
        </Select>
      </SettingsRow>

      <SettingsRow
        label="Data storage locations"
        helper={
          <>
            <span className="block break-all font-mono text-xs">{storageLocations?.recordings || 'Loading...'}</span>
            <span className="mt-2 block">Database and models are stored together in your application data directory for unified management.</span>
          </>
        }
      >
        <Button variant="outline" size="sm" onClick={() => handleOpenFolder('recordings')}>
          <FolderOpen className="mr-2 h-4 w-4" />
          Open Folder
        </Button>
      </SettingsRow>
    </div>
  )
}
