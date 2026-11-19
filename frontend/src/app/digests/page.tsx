'use client';

import { Suspense, useEffect, useState } from 'react';
import { useSearchParams } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { DigestView, useDigest } from '@/components/Briefs/DigestView';

type Item = { id: string; kind: 'brief' | 'digest'; title: string; createdAt: string };

function formatDate(date: string): string {
  const d = new Date(date);
  return Number.isNaN(d.getTime()) ? date : d.toLocaleDateString();
}

// Task 44 moves these states into the page layout
function SelectedDigest({ id }: { id: string }) {
  const { state, digest } = useDigest(id);
  if (state === 'loading') return <p className="text-sm text-muted-foreground">Loading…</p>;
  if (state === 'error') return <p role="alert" className="text-sm text-red-600">Could not load this digest.</p>;
  if (state === 'missing' || !digest) return <p className="text-sm text-muted-foreground">This digest is no longer there.</p>;
  return <DigestView digest={digest} from={{ path: `/digests?id=${id}`, label: 'Digests' }} />;
}

function DigestsContent() {
  const searchParams = useSearchParams();
  const urlId = searchParams.get('id');
  const [items, setItems] = useState<Item[]>([]);
  const [selected, setSelected] = useState<string | null>(urlId);
  const [error, setError] = useState<string | null>(null);

  // A feed click on a digest while this page is open changes the query, so follow it.
  useEffect(() => {
    setSelected(urlId);
  }, [urlId]);

  useEffect(() => {
    let live = true;
    invoke<Item[]>('briefs_list')
      .then((list) => {
        if (live) setItems(list);
      })
      .catch(() => {
        // The error text can echo brief content, so it is not logged
        console.error('Digests failed to load');
        if (live) setError('Could not load digests.');
      });
    return () => {
      live = false;
    };
  }, []);

  const shown = items.filter((item) => item.kind === 'digest');

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-3xl space-y-6 p-8">
        <h1 className="text-3xl font-bold">Digests</h1>

        {error && <p role="alert" className="text-sm text-red-600">{error}</p>}

        {shown.length === 0 ? (
          <p className="text-sm text-muted-foreground">No digests yet.</p>
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

        {selected && <SelectedDigest key={selected} id={selected} />}
      </div>
    </div>
  );
}

export default function DigestsPage() {
  return (
    <Suspense fallback={null}>
      <DigestsContent />
    </Suspense>
  );
}
