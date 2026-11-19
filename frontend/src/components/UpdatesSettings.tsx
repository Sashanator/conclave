'use client';

import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { SettingsRow, useSavedTick } from '@/components/Settings/SettingsRow';
import { formatRowDate } from '@/lib/dates';
import { formatMegabytes } from '@/lib/old-audio';
import {
  TOKEN_HELP, backupLabel, backupList, backupNow, backupOpenFolder, checkMessage, lastCheckLine, testMessage,
  updatesCheck, updatesGetStatus, updatesRemoveToken, updatesSetCheckOnStart, updatesSetToken,
  type BackupInfo, type UpdatesStatus,
} from '@/lib/updates';

type Msg = { text: string; error: boolean };
const line = (m: Msg) => <p className={`text-sm ${m.error ? 'text-destructive' : 'text-muted-foreground'}`}>{m.text}</p>;

export function UpdatesSettings() {
  const [status, setStatus] = useState<UpdatesStatus | null>(null);
  const [statusFailed, setStatusFailed] = useState(false);
  const [token, setToken] = useState('');
  const [busy, setBusy] = useState(false);
  const [tokenMsg, setTokenMsg] = useState<Msg | null>(null);
  const [checking, setChecking] = useState(false);
  const [checkMsg, setCheckMsg] = useState<Msg | null>(null);
  const [backups, setBackups] = useState<BackupInfo[] | null>(null);
  const [listFailed, setListFailed] = useState(false);
  const [backingUp, setBackingUp] = useState(false);

  const loadStatus = useCallback(async () => {
    try { setStatus(await updatesGetStatus()); setStatusFailed(false); } catch { setStatusFailed(true); }
  }, []);
  const loadBackups = useCallback(async () => {
    try { setBackups(await backupList()); setListFailed(false); } catch { setListFailed(true); }
  }, []);
  useEffect(() => { void loadStatus(); void loadBackups(); }, [loadStatus, loadBackups]);

  const saveToken = async () => {
    setBusy(true); setTokenMsg(null);
    try { await updatesSetToken(token.trim()); setToken(''); markTokenSaved(); await loadStatus(); }
    catch (e) { setTokenMsg({ text: String(e), error: true }); }
    finally { setBusy(false); }
  };
  const testToken = async () => {
    setBusy(true); setTokenMsg(null);
    try { setTokenMsg(testMessage(await updatesCheck())); }
    catch (e) { setTokenMsg({ text: String(e), error: true }); }
    finally { setBusy(false); await loadStatus(); }
  };
  const removeToken = async () => {
    setBusy(true); setTokenMsg(null);
    try { await updatesRemoveToken(); await loadStatus(); }
    catch (e) { setTokenMsg({ text: String(e), error: true }); }
    finally { setBusy(false); }
  };
  const [toggling, setToggling] = useState(false);
  const [saved, markSaved] = useSavedTick();
  const [tokenSaved, markTokenSaved] = useSavedTick();
  const toggle = async (enabled: boolean) => {
    setToggling(true);
    setStatus((s) => s && { ...s, checkOnStart: enabled });
    try { await updatesSetCheckOnStart(enabled); markSaved(); }
    catch (e) {
      setStatus((s) => s && { ...s, checkOnStart: !enabled });
      toast.error('Could not save the setting', { description: String(e) });
    } finally { setToggling(false); }
  };
  const check = async () => {
    setChecking(true); setCheckMsg(null);
    try { setCheckMsg(checkMessage(await updatesCheck())); }
    catch (e) { setCheckMsg({ text: String(e), error: true }); }
    finally { setChecking(false); await loadStatus(); }
  };
  const backUp = async () => {
    setBackingUp(true);
    try {
      const b = await backupNow();
      toast.success(`Backed up (${formatMegabytes(b.sizeBytes)})`);
      await loadBackups();
    } catch (e) { toast.error('Backup failed', { description: String(e) }); }
    finally { setBackingUp(false); }
  };
  const openFolder = async () => {
    try { await backupOpenFolder(); }
    catch (e) { toast.error('Could not open the backups folder', { description: String(e) }); }
  };

  return (
    <div>
      {statusFailed && <p className="py-2 text-sm text-destructive">Could not load update settings</p>}
      {status && (
        <>
          <SettingsRow label="Updates" helper={`Current version v${status.currentVersion}. ${lastCheckLine(status)}`}>
            <Button size="sm" onClick={check} disabled={checking}>Check for updates</Button>
            {checkMsg && <div className="mt-2">{line(checkMsg)}</div>}
          </SettingsRow>

          <SettingsRow label="Check for updates on start" htmlFor="update-check-on-start" saved={saved}>
            <Switch id="update-check-on-start" checked={status.checkOnStart} onCheckedChange={toggle} disabled={toggling} aria-label="Check for updates on start" />
          </SettingsRow>

          <SettingsRow label="GitHub token" saved={tokenSaved} htmlFor={status.tokenLast4 === null ? 'update-token' : undefined} helper={TOKEN_HELP} block>
            {status.tokenLast4 === null ? (
              <div className="flex flex-wrap items-center gap-2">
                <Input id="update-token" type="password" autoComplete="off" className="w-72" value={token} onChange={(e) => setToken(e.target.value)} />
                <Button size="sm" onClick={saveToken} disabled={!token.trim() || busy}>Save</Button>
              </div>
            ) : (
              <div className="flex flex-wrap items-center gap-3">
                <span className="text-sm">Token saved ••••{status.tokenLast4}</span>
                <Button size="sm" variant="outline" onClick={testToken} disabled={busy}>Test</Button>
                <Button size="sm" variant="outline" onClick={removeToken} disabled={busy}>Remove</Button>
              </div>
            )}
            {tokenMsg && <div className="mt-2">{line(tokenMsg)}</div>}
          </SettingsRow>
        </>
      )}

      <SettingsRow label="Backups" block>
        <div className="flex flex-wrap gap-3">
          <Button size="sm" onClick={backUp} disabled={backingUp}>{backingUp ? 'Backing up…' : 'Back up now'}</Button>
          <Button size="sm" variant="outline" onClick={openFolder}>Open backups folder</Button>
        </div>
        {listFailed ? <p className="mt-3 text-sm text-destructive">Could not list backups</p>
          : backups && backups.length === 0 ? <p className="mt-3 text-sm text-muted-foreground">No backups yet</p>
          : <ul className="mt-3 space-y-1 text-sm">
              {backups?.map((b) => (
                <li key={b.path} className="flex flex-wrap gap-x-3">
                  <span>{formatRowDate(new Date(b.createdAt))}</span>
                  <span>{backupLabel(b)}</span>
                  <span className="text-muted-foreground">{formatMegabytes(b.sizeBytes)}</span>
                </li>
              ))}
            </ul>}
      </SettingsRow>
    </div>
  );
}
