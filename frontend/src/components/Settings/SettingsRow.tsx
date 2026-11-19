'use client';

import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { settingId } from '@/lib/settings-panes';

// The tick shows for 2 s; each markSaved() restarts it.
export function useSavedTick(): [saved: boolean, markSaved: () => void] {
  const [saved, setSaved] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>();
  const markSaved = useCallback(() => {
    clearTimeout(timer.current);
    setSaved(true);
    timer.current = setTimeout(() => setSaved(false), 2000);
  }, []);
  useEffect(() => () => clearTimeout(timer.current), []);
  return [saved, markSaved];
}

export function SettingsRow({ label, helper, htmlFor, saved, block, children }: {
  label: string;
  helper?: ReactNode;
  htmlFor?: string;
  saved?: boolean;
  block?: boolean;
  children?: ReactNode;
}) {
  const title = htmlFor ? (
    <label htmlFor={htmlFor} className="text-sm font-medium text-foreground">{label}</label>
  ) : (
    <span className="text-sm font-medium text-foreground">{label}</span>
  );
  return (
    <div id={settingId(label)} className={`border-b border-border py-4 last:border-b-0 ${block ? 'space-y-3' : 'flex flex-wrap items-start justify-between gap-x-6 gap-y-2'}`}>
      <div className="min-w-0 flex-1 basis-64">
        <div className="flex items-center gap-2">
          {title}
          <span role="status" aria-live="polite" className="text-xs text-muted-foreground">{saved ? 'Saved ✓' : null}</span>
        </div>
        {helper ? <p className="mt-1 text-sm text-muted-foreground">{helper}</p> : null}
      </div>
      {children ? <div className={block ? 'w-full' : 'shrink-0'}>{children}</div> : null}
    </div>
  );
}
