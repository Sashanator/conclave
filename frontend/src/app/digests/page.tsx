'use client';

import { Suspense, useEffect, useState } from 'react';
import { useSearchParams } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { PageFrame } from '@/components/PageFrame/PageFrame';
import { DigestView, MeetingChip, useDigest } from '@/components/Briefs/DigestView';
import { formatLongDate } from '@/lib/dates';

type Item = { id: string; kind: 'brief' | 'digest'; title: string; createdAt: string };

function DigestsContent() {
  const searchParams = useSearchParams();
  const urlId = searchParams.get('id');
  const [items, setItems] = useState<Item[]>([]);
  const [picked, setPicked] = useState<string | null>(urlId);
  const [error, setError] = useState<string | null>(null);

  // A feed click on a digest while this page is open changes the query, so follow it.
  useEffect(() => {
    setPicked(urlId);
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
  const selected = picked ?? shown[0]?.id ?? null;
  const { state, digest } = useDigest(selected);
  const from = { path: `/digests?id=${selected}`, label: 'Digests' };
  const dateOf = (v: string) => {
    const d = new Date(v);
    return Number.isNaN(d.getTime()) ? v : formatLongDate(d);
  };

  const list =
    shown.length === 0 ? (
      <p className="text-sm text-muted-foreground">No digests yet.</p>
    ) : (
      <ul className="divide-y divide-border">
        {shown.map((item) => (
          <li key={item.id}>
            <button
              type="button"
              aria-pressed={selected === item.id}
              onClick={() => setPicked(item.id)}
              className={`block w-full rounded-md px-3 py-2 text-left ${selected === item.id ? 'bg-muted' : 'hover:bg-accent'}`}
            >
              <span className="block truncate">{item.title}</span>
              <span className={`block text-sm ${selected === item.id ? 'text-foreground/75' : 'text-muted-foreground'}`}>{dateOf(item.createdAt)}</span>
            </button>
          </li>
        ))}
      </ul>
    );

  const rail = digest && (
    <section aria-labelledby="digest-meetings" className="space-y-2">
      <h2 id="digest-meetings" className="text-sm font-medium text-muted-foreground">Meetings in this digest</h2>
      <div className="flex flex-col items-start gap-2">
        {digest.meetingLinks.map((m, i) => (
          <MeetingChip key={`${m.id}-${i}`} meeting={m} from={from} />
        ))}
      </div>
    </section>
  );

  return (
    <PageFrame variant="reading" title="Digests" subnav={list} rail={rail}>
      {error && <p role="alert" className="text-sm text-red-600">{error}</p>}
      {selected && state === 'loading' && <p className="text-sm text-muted-foreground">Loading…</p>}
      {selected && state === 'error' && <p role="alert" className="text-sm text-red-600">Could not load this digest.</p>}
      {selected && state === 'missing' && <p className="text-sm text-muted-foreground">This digest is no longer there.</p>}
      {state === 'ready' && digest && <DigestView digest={digest} from={from} />}
    </PageFrame>
  );
}

export default function DigestsPage() {
  return (
    <Suspense fallback={null}>
      <DigestsContent />
    </Suspense>
  );
}
