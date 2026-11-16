'use client';

import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { formatMegabytes } from '@/lib/old-audio';
import {
  TOKEN_HELP, backupLabel, backupList, backupNow, backupOpenFolder, checkMessage, lastCheckLine, testMessage,
  updatesCheck, updatesGetStatus, updatesRemoveToken, updatesSetCheckOnStart, updatesSetToken,
  type BackupInfo, type UpdatesStatus,
} from '@/lib/updates';

type Msg = { text: string; error: boolean };
const card = 'bg-card rounded-lg border border-border p-6 shadow-sm space-y-4';
const line = (m: Msg) => <p className={`text-sm ${m.error ? 'text-red-600' : 'text-muted-foreground'}`}>{m.text}</p>;

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
    try { await updatesSetToken(token.trim()); setToken(''); await loadStatus(); }
    catch (e) { setTokenMsg({ text: String(e), error: true }); }
    finally { setBusy(false); }
  };
  const testToken = async () => {
    setBusy(true); setTokenMsg(null);
    try { setTokenMsg(testMessage(await updatesCheck())); }
    catch (e) { setTokenMsg({ text: String(e), error: true }); }
    finally { setBusy(false); }
  };
  const removeToken = async () => {
    setBusy(true); setTokenMsg(null);
    try { await updatesRemoveToken(); await loadStatus(); }
    catch (e) { setTokenMsg({ text: String(e), error: true }); }
    finally { setBusy(false); }
  };
  const toggle = async (enabled: boolean) => {
    setStatus((s) => s && { ...s, checkOnStart: enabled });
    try { await updatesSetCheckOnStart(enabled); }
    catch (e) {
      setStatus((s) => s && { ...s, checkOnStart: !enabled });
      toast.error('Could not save the setting', { description: String(e) });
    }
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
    <div className="space-y-6">
      <section className={card}>
        <h3 className="text-lg font-semibold">Updates</h3>
        {statusFailed && <p className="text-sm text-red-600">Could not load update settings</p>}
        {status && (
          <>
            <p className="text-sm text-muted-foreground">Current version v{status.currentVersion}</p>
            {status.tokenLast4 === null ? (
              <div className="space-y-2">
                <Label htmlFor="update-token">GitHub token</Label>
                <Input id="update-token" type="password" autoComplete="off" value={token} onChange={(e) => setToken(e.target.value)} />
                <Button size="sm" onClick={saveToken} disabled={!token.trim() || busy}>Save</Button>
              </div>
            ) : (
              <div className="flex flex-wrap items-center gap-3">
                <span className="text-sm">Token saved ••••{status.tokenLast4}</span>
                <Button size="sm" variant="outline" onClick={testToken} disabled={busy}>Test</Button>
                <Button size="sm" variant="outline" onClick={removeToken} disabled={busy}>Remove</Button>
              </div>
            )}
            {tokenMsg && line(tokenMsg)}
            <p className="text-xs text-muted-foreground">{TOKEN_HELP}</p>
            <div className="flex items-center gap-3">
              <Switch checked={status.checkOnStart} onCheckedChange={toggle} aria-label="Check for updates on start" />
              <span className="text-sm">Check for updates on start</span>
            </div>
            <Button size="sm" onClick={check} disabled={checking}>Check for updates</Button>
            {checkMsg && line(checkMsg)}
            <p className="text-sm text-muted-foreground">{lastCheckLine(status)}</p>
          </>
        )}
      </section>

      <section className={card}>
        <h3 className="text-lg font-semibold">Backups</h3>
        <div className="flex flex-wrap gap-3">
          <Button size="sm" onClick={backUp} disabled={backingUp}>{backingUp ? 'Backing up…' : 'Back up now'}</Button>
          <Button size="sm" variant="outline" onClick={openFolder}>Open backups folder</Button>
        </div>
        {listFailed ? <p className="text-sm text-red-600">Could not list backups</p>
          : backups && backups.length === 0 ? <p className="text-sm text-muted-foreground">No backups yet</p>
          : <ul className="space-y-1 text-sm">
              {backups?.map((b) => (
                <li key={b.path} className="flex flex-wrap gap-x-3">
                  <span>{new Date(b.createdAt).toLocaleString()}</span>
                  <span>{backupLabel(b)}</span>
                  <span className="text-muted-foreground">{formatMegabytes(b.sizeBytes)}</span>
                </li>
              ))}
            </ul>}
      </section>
    </div>
  );
}
