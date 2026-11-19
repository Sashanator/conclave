'use client';

import React, { useState } from 'react';
import { SETTINGS_GROUPS, paneLabel, searchSettings, type SettingsPane } from '@/lib/settings-panes';
import { Input } from '@/components/ui/input';

export function SettingsNav({ pane, onPick }: { pane: SettingsPane; onPick: (p: SettingsPane, label?: string) => void }) {
  const [q, setQ] = useState('');
  const results = searchSettings(q);
  return (
    <nav aria-label="Settings" className="space-y-4">
      <Input aria-label="Search settings" placeholder="Search settings" value={q} onChange={(e) => setQ(e.target.value)} />
      {q.trim() ? (
        results.length ? (
          <ul className="space-y-1">
            {results.map((r) => (
              <li key={`${r.pane}-${r.label}`}>
                <button type="button" onClick={() => { onPick(r.pane, r.label); setQ(''); }} className="w-full rounded-md px-2 py-1.5 text-left text-sm hover:bg-accent">
                  {r.label} <span className="text-muted-foreground">{paneLabel(r.pane)}</span>
                </button>
              </li>
            ))}
          </ul>
        ) : <p className="px-2 text-sm text-muted-foreground">No settings match</p>
      ) : (
        SETTINGS_GROUPS.map((g) => (
          <div key={g.label}>
            <div className="px-2 pb-1 text-xs font-medium uppercase tracking-wide text-muted-foreground">{g.label}</div>
            <ul className="space-y-0.5">
              {g.panes.map((p) => (
                <li key={p.id}>
                  <button type="button" aria-current={p.id === pane ? 'page' : undefined} onClick={() => onPick(p.id)}
                    className={`w-full rounded-md px-2 py-1.5 text-left text-sm hover:bg-accent ${p.id === pane ? 'bg-accent font-medium' : 'text-muted-foreground'}`}>
                    {p.label}
                  </button>
                </li>
              ))}
            </ul>
          </div>
        ))
      )}
    </nav>
  );
}
