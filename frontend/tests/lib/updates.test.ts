import { afterAll, describe, expect, mock, test } from 'bun:test';

const originals = { core: { ...await import('@tauri-apps/api/core') } };
const calls: Array<[string, unknown]> = [];
mock.module('@tauri-apps/api/core', () => ({
  invoke: async (cmd: string, args?: unknown) => { calls.push([cmd, args]); return null; },
}));
// Bun shares module mocks between test files; restore the real core module after this suite.
afterAll(() => { mock.module('@tauri-apps/api/core', () => originals.core); });

const lib = await import('../../src/lib/updates');
const { checkMessage, testMessage, installError, progressLabel, backupLabel, lastCheckLine } = lib;

describe('updates lib', () => {
  test('checkMessage maps every status', () => {
    expect(checkMessage({ status: 'not_set_up', version: null, notes: null, message: null })).toEqual({ text: 'Add a GitHub token first.', error: false });
    expect(checkMessage({ status: 'up_to_date', version: null, notes: null, message: null })).toEqual({ text: 'Conclave is up to date.', error: false });
    expect(checkMessage({ status: 'available', version: '1.2.0', notes: null, message: null })).toEqual({ text: 'Conclave v1.2.0 is available.', error: false });
    expect(checkMessage({ status: 'error', version: null, notes: null, message: 'Offline' })).toEqual({ text: 'Offline', error: true });
    expect(checkMessage({ status: 'error', version: null, notes: null, message: null })).toEqual({ text: 'Check failed.', error: true });
  });

  test('testMessage reports Works for up_to_date and available', () => {
    expect(testMessage({ status: 'up_to_date', version: null, notes: null, message: null })).toEqual({ text: 'Works', error: false });
    expect(testMessage({ status: 'available', version: '1.2.0', notes: null, message: null })).toEqual({ text: 'Works', error: false });
    expect(testMessage({ status: 'not_set_up', version: null, notes: null, message: null })).toEqual({ text: 'No token saved.', error: true });
    expect(testMessage({ status: 'error', version: null, notes: null, message: 'Bad token' })).toEqual({ text: 'Bad token', error: true });
  });

  test('installError recognises busy', () => {
    expect(installError('busy: a recording is in progress or being saved')).toContain('busy (a recording is in progress or being saved)');
    expect(installError('bad signature')).toBe('Update failed: bad signature. Nothing was changed.');
  });

  test('progressLabel percent, done and unknown total', () => {
    expect(progressLabel({ downloaded: 50, total: 100 })).toBe('Downloading… 50%');
    expect(progressLabel({ downloaded: 100, total: 100 })).toBe('Installing… Conclave will restart.');
    expect(progressLabel({ downloaded: 12_400_000, total: null })).toBe('Downloading… 12 MB');
  });

  test('backupLabel both kinds', () => {
    const base = { createdAt: '2026-11-16T08:59:00Z', sizeBytes: 1, path: 'x' };
    expect(backupLabel({ ...base, kind: 'pre-upgrade', fromVersion: '1.0.4', version: '1.1.0' })).toBe('Before upgrade v1.0.4 → v1.1.0');
    expect(backupLabel({ ...base, kind: 'pre-upgrade', fromVersion: null, version: '1.1.0' })).toBe('Before upgrade v? → v1.1.0');
    expect(backupLabel({ ...base, kind: 'manual', fromVersion: null, version: '1.1.0' })).toBe('Manual v1.1.0');
  });

  test('lastCheckLine never checked', () => {
    expect(lastCheckLine({ tokenLast4: null, checkOnStart: true, lastCheck: null, currentVersion: '1.1.0' })).toBe('Not checked yet');
    const line = lastCheckLine({ tokenLast4: null, checkOnStart: true, lastCheck: { at: '2026-11-16T09:00:00Z', result: 'Up to date' }, currentVersion: '1.1.0' });
    expect(line).toBe(`Last checked ${new Date('2026-11-16T09:00:00Z').toLocaleString()}: Up to date`);
  });

  test('wrappers call the pinned Rust command names with their args', async () => {
    calls.length = 0;
    await lib.updatesGetStatus();
    await lib.updatesSetToken('ghp_x');
    await lib.updatesRemoveToken();
    await lib.updatesSetCheckOnStart(false);
    await lib.updatesCheck();
    await lib.updatesInstall();
    await lib.backupNow();
    await lib.backupList();
    await lib.backupOpenFolder();
    expect(calls).toEqual([
      ['updates_get_status', undefined],
      ['updates_set_token', { token: 'ghp_x' }],
      ['updates_remove_token', undefined],
      ['updates_set_check_on_start', { enabled: false }],
      ['updates_check', undefined],
      ['updates_install', undefined],
      ['backup_now', undefined],
      ['backup_list', undefined],
      ['backup_open_folder', undefined],
    ]);
  });
});
