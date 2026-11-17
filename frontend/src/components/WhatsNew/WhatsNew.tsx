'use client';

import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import { toast } from 'sonner';
import { parseChangelog } from '@/lib/changelog';
import { WHATS_NEW_SEEN_KEY as SEEN_KEY } from '@/lib/nav';

// `**x**` becomes <b>; everything else stays text
function inline(s: string) {
  return s.split(/\*\*(.+?)\*\*/g).map((p, i) => (i % 2 ? <b key={i}>{p}</b> : p));
}

export function WhatsNew({ changelog }: { changelog: string }) {
  const releases = useMemo(() => parseChangelog(changelog), [changelog]);
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    getVersion()
      .then((v) => {
        if (!live) return;
        setVersion(v);
        try {
          localStorage.setItem(SEEN_KEY, v);
        } catch {}
        window.dispatchEvent(new Event('whats-new-seen'));
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  const open = (command: string, what: string) => () =>
    invoke(command).catch((e) => toast.error(`Could not open the ${what}: ${e}`));

  return (
    <div className="mx-auto max-w-2xl p-6">
      <h1 className="text-xl font-semibold mb-4">What&apos;s new</h1>
      {releases.map((r) => (
        <article key={r.version} className="mb-6" aria-current={r.version === version ? 'true' : undefined}>
          <h2 className="font-medium">
            v{r.version} <span className="text-sm text-muted-foreground">{r.date}</span>
            {r.version === version && <span className="ml-2 text-sm">Installed</span>}
          </h2>
          <ul className="list-disc pl-5 mt-1">
            {r.items.map((it, i) => (
              <li key={i}>{inline(it)}</li>
            ))}
          </ul>
        </article>
      ))}
      <footer className="text-sm text-muted-foreground border-t pt-3">
        Conclave v{version ?? ''} · Based on Meetily (MIT) by Zackriya Solutions ·{' '}
        <button className="underline" onClick={open('open_database_folder', 'data folder')}>Open data folder</button>{' · '}
        <button className="underline" onClick={open('open_log_file', 'log file')}>Open log file</button>
      </footer>
    </div>
  );
}
