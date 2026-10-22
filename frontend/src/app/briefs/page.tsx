'use client';

import { Suspense, useEffect, useState } from 'react';
import { useSearchParams } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/button';
import { BriefView } from '@/components/Briefs/BriefView';

type Kind = 'brief' | 'digest';
type Item = { id: string; kind: Kind; title: string; createdAt: string };

function formatDate(date: string): string {
  const d = new Date(date);
  return Number.isNaN(d.getTime()) ? date : d.toLocaleDateString();
}

function BriefsContent() {
  const searchParams = useSearchParams();
  const urlId = searchParams.get('id');
  const urlKind = searchParams.get('kind');
  const [kind, setKind] = useState<Kind>(urlKind === 'digest' ? 'digest' : 'brief');
  const [items, setItems] = useState<Item[]>([]);
  const [selected, setSelected] = useState<string | null>(urlId);
  const [error, setError] = useState<string | null>(null);

  // A feed click on a brief while this page is open changes the query, so follow it.
  useEffect(() => {
    setSelected(urlId);
  }, [urlId]);
  useEffect(() => {
    if (urlKind) setKind(urlKind === 'digest' ? 'digest' : 'brief');
  }, [urlKind]);

  useEffect(() => {
    let live = true;
    invoke<Item[]>('briefs_list')
      .then((list) => {
        if (live) setItems(list);
      })
      .catch(() => {
        // The error text can echo brief content, so it is not logged
        console.error('Briefs failed to load');
        if (live) setError('Could not load briefs.');
      });
    return () => {
      live = false;
    };
  }, []);

  const shown = items.filter((item) => item.kind === kind);

  return (
    <div className="h-full overflow-y-auto bg-muted/50">
      <div className="mx-auto max-w-3xl space-y-6 p-8">
        <h1 className="text-3xl font-bold">Briefs & digests</h1>

        <div className="flex items-center gap-2">
          <Button variant={kind === 'brief' ? 'secondary' : 'ghost'} aria-pressed={kind === 'brief'} onClick={() => setKind('brief')}>
            Briefs
          </Button>
          <Button variant={kind === 'digest' ? 'secondary' : 'ghost'} aria-pressed={kind === 'digest'} onClick={() => setKind('digest')}>
            Digests
          </Button>
        </div>

        {error && <p role="alert" className="text-sm text-red-600">{error}</p>}

        {shown.length === 0 ? (
          <p className="text-sm text-muted-foreground">{kind === 'brief' ? 'No briefs yet.' : 'No digests yet.'}</p>
        ) : (
          <ul className="divide-y divide-border rounded-lg border border-border bg-card">
            {shown.map((item) => (
              <li key={item.id}>
                <button
                  type="button"
                  aria-pressed={selected === item.id}
                  onClick={() => setSelected(item.id)}
                  className={`flex w-full items-baseline justify-between gap-3 px-4 py-2 text-left ${selected === item.id ? 'bg-muted' : 'hover:bg-accent'}`}
                >
                  <span>{item.title}</span>
                  <span className="text-sm text-muted-foreground">{formatDate(item.createdAt)}</span>
                </button>
              </li>
            ))}
          </ul>
        )}

        {selected && kind === 'brief' && <BriefView key={selected} id={selected} />}
      </div>
    </div>
  );
}

export default function BriefsPage() {
  return (
    <Suspense fallback={null}>
      <BriefsContent />
    </Suspense>
  );
}
