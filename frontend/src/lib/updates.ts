import { invoke } from '@tauri-apps/api/core';
import { formatMegabytes } from '@/lib/old-audio';

export interface LastCheck { at: string; result: string }
export interface UpdatesStatus { tokenLast4: string | null; checkOnStart: boolean; lastCheck: LastCheck | null; currentVersion: string }
export interface UpdateCheckResult { status: 'not_set_up' | 'up_to_date' | 'available' | 'error'; version: string | null; notes: string | null; message: string | null }
export interface UpdateAvailable { version: string; notes: string | null }
export interface UpdateProgress { downloaded: number; total: number | null }
export interface BackupInfo { kind: 'pre-upgrade' | 'manual'; createdAt: string; fromVersion: string | null; version: string; sizeBytes: number; path: string }
export const UPDATE_AVAILABLE_EVENT = 'update-available';
export const UPDATES_PROGRESS_EVENT = 'updates-progress';
export const TOKEN_HELP = 'Create a fine-grained token with read-only Contents access to the Conclave repository only.';

export const updatesGetStatus = () => invoke<UpdatesStatus>('updates_get_status');
export const updatesSetToken = (token: string) => invoke<void>('updates_set_token', { token });
export const updatesRemoveToken = () => invoke<void>('updates_remove_token');
export const updatesSetCheckOnStart = (enabled: boolean) => invoke<void>('updates_set_check_on_start', { enabled });
export const updatesCheck = () => invoke<UpdateCheckResult>('updates_check');
export const updatesInstall = () => invoke<void>('updates_install');
export const backupNow = () => invoke<BackupInfo>('backup_now');
export const backupList = () => invoke<BackupInfo[]>('backup_list');
export const backupOpenFolder = () => invoke<void>('backup_open_folder');

export function checkMessage(r: UpdateCheckResult): { text: string; error: boolean } {
  switch (r.status) {
    case 'not_set_up': return { text: 'Add a GitHub token first.', error: false };
    case 'up_to_date': return { text: 'Conclave is up to date.', error: false };
    case 'available': return { text: `Conclave v${r.version} is available.`, error: false };
    case 'error': return { text: r.message ?? 'Check failed.', error: true };
  }
}

export function testMessage(r: UpdateCheckResult): { text: string; error: boolean } {
  switch (r.status) {
    case 'up_to_date':
    case 'available': return { text: 'Works', error: false };
    case 'error': return { text: r.message ?? 'Check failed.', error: true };
    case 'not_set_up': return { text: 'No token saved.', error: true };
  }
}

export function installError(e: unknown): string {
  const s = String(e);
  return s.startsWith('busy: ')
    ? `Conclave is busy (${s.slice('busy: '.length)}). Update when it finishes.`
    : `Update failed: ${s.trim().replace(/\.$/, '')}. Nothing was changed.`;
}

export function progressLabel(p: UpdateProgress): string {
  if (p.total !== null && p.downloaded >= p.total) return 'Installing… Conclave will restart.';
  if (p.total === null) return `Downloading… ${formatMegabytes(p.downloaded)}`;
  return `Downloading… ${Math.floor((p.downloaded / p.total) * 100)}%`;
}

export function backupLabel(b: BackupInfo): string {
  return b.kind === 'pre-upgrade'
    ? `Before upgrade v${b.fromVersion ?? '?'} → v${b.version}`
    : `Manual v${b.version}`;
}

export function lastCheckLine(s: UpdatesStatus): string {
  return s.lastCheck
    ? `Last checked ${new Date(s.lastCheck.at).toLocaleString()}: ${s.lastCheck.result}`
    : 'Not checked yet';
}
