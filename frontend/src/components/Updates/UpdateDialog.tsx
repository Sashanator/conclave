'use client';
import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Progress } from '@/components/ui/progress';
import {
  UPDATE_AVAILABLE_EVENT, UPDATES_PROGRESS_EVENT, installError, progressLabel, updatesInstall,
  type UpdateAvailable, type UpdateProgress,
} from '@/lib/updates';

export function UpdateDialog() {
  const [update, setUpdate] = useState<UpdateAvailable | null>(null);
  const [open, setOpen] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Events are subscribed once; the ref keeps their handlers current without resubscribing.
  const installingRef = useRef(false);

  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];
    const sub = async <T,>(event: string, handler: (payload: T) => void) => {
      const un = await listen<T>(event, (e) => handler(e.payload));
      if (cancelled) un(); else unlisteners.push(un);
    };
    sub<UpdateAvailable>(UPDATE_AVAILABLE_EVENT, (p) => {
      if (installingRef.current) return;
      setUpdate({ version: p.version, notes: p.notes });
      setError(null);
      setProgress(null);
      setOpen(true);
    }).catch((e) => console.error('update dialog: could not subscribe', e));
    sub<UpdateProgress>(UPDATES_PROGRESS_EVENT, (p) => { if (installingRef.current) setProgress(p); }).catch((e) => console.error('update dialog: could not subscribe', e));
    return () => { cancelled = true; unlisteners.forEach((un) => un()); };
  }, []);

  const install = async () => {
    installingRef.current = true;
    setInstalling(true);
    setError(null);
    setProgress(null);
    try {
      await updatesInstall();
      // Stay in the installing state: the app restarts itself.
    } catch (e) {
      installingRef.current = false;
      setInstalling(false);
      setProgress(null);
      setError(installError(e));
    }
  };

  const percent = progress && progress.total ? Math.min(100, (progress.downloaded / progress.total) * 100) : undefined;

  return (
    <Dialog open={open} onOpenChange={(next) => { if (!installingRef.current) setOpen(next); }}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>Conclave v{update?.version} is available</DialogTitle>
          <DialogDescription>Updating downloads the new version and restarts Conclave.</DialogDescription>
        </DialogHeader>
        {update?.notes && <p className="whitespace-pre-wrap max-h-64 overflow-y-auto text-sm">{update.notes}</p>}
        {installing && (
          <div className="space-y-2">
            <p className="text-sm">{progress ? progressLabel(progress) : 'Starting…'}</p>
            <Progress value={percent} />
          </div>
        )}
        {error && <p className="text-sm text-red-600">{error}</p>}
        <DialogFooter>
          <Button variant="outline" disabled={installing} onClick={() => setOpen(false)}>Later</Button>
          <Button disabled={installing} onClick={install}>Update</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
