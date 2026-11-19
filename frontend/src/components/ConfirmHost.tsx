'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { setConfirmHandler, type ConfirmRequest } from '@/lib/confirm';
import { DESTRUCTIVE_OUTLINE } from '@/lib/utils';

/** Mounted once in the app layout; renders every `confirmAction` request. */
export function ConfirmHost() {
  const pending = useRef<ConfirmRequest | null>(null);
  const [req, setReq] = useState<ConfirmRequest | null>(null);

  const close = useCallback((ok: boolean) => {
    const r = pending.current;
    pending.current = null;
    setReq(null);
    r?.resolve(ok);
  }, []);

  useEffect(() => {
    setConfirmHandler((next) => {
      pending.current?.resolve(false);
      pending.current = next;
      setReq(next);
    });
    return () => {
      setConfirmHandler(null);
      pending.current?.resolve(false);
      pending.current = null;
    };
  }, []);

  if (!req) return null;
  return (
    <Dialog open onOpenChange={(open) => { if (!open) close(false); }}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{req.title}</DialogTitle>
          {req.body && <DialogDescription>{req.body}</DialogDescription>}
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={() => close(false)}>Cancel</Button>
          <Button
            variant={req.destructive ? 'outline' : 'default'}
            className={req.destructive ? DESTRUCTIVE_OUTLINE : undefined}
            onClick={() => close(true)}
          >
            {req.confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
