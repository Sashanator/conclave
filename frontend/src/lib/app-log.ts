import { invoke } from '@tauri-apps/api/core';

// A line in the local app log (never sent anywhere). Status text only: no transcript, summary text or keys.
export function appLog(level: 'info' | 'warn' | 'error', message: string): void {
  void invoke('app_log', { level, message }).catch(() => {});
}
