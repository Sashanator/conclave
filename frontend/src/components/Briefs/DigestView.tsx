'use client';

import { useEffect, useState, type ReactNode } from 'react';
import Link from 'next/link';
import { invoke } from '@tauri-apps/api/core';
import ReactMarkdown from 'react-markdown';
import { meetingHref } from '@/lib/meeting-target';

export type DigestContent = {
  periodStart: string;
  periodEnd: string;
  markdown: string;
  meetingLinks: { id: string; title: string }[];
};

// The model's [M3] tags become links to the digest's meeting, through a relative href that carries the tag number.
const TAG = /\[M(\d+)\]/g;
const TAG_HREF = /^digest-tag-(\d+)$/;

function withTagLinks(markdown: string): string {
  return markdown.replace(TAG, (_, n: string) => `[M${n}](digest-tag-${n})`);
}

// Shown as given when it is not a date, so the text is never lost.
function formatDay(value: string): string {
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? value : d.toLocaleDateString();
}

export function DigestView({ id }: { id: string }) {
  const [digest, setDigest] = useState<DigestContent | null>(null);
  const [state, setState] = useState<'loading' | 'ready' | 'missing' | 'error'>('loading');

  useEffect(() => {
    let live = true;
    setState('loading');
    invoke<DigestContent | null>('digest_get', { id })
      .then((content) => {
        if (!live) return;
        setDigest(content);
        setState(content ? 'ready' : 'missing');
      })
      .catch(() => {
        // The error text can echo digest content, so it is not logged
        console.error('Digest failed to load');
        if (live) setState('error');
      });
    return () => {
      live = false;
    };
  }, [id]);

  if (state === 'loading') return <p className="text-sm text-muted-foreground">Loading…</p>;
  if (state === 'error') return <p role="alert" className="text-sm text-red-600">Could not load this digest.</p>;
  if (state === 'missing' || !digest) return <p className="text-sm text-muted-foreground">This digest is no longer there.</p>;

  const links = digest.meetingLinks;
  // Only our [Mn] tags link anywhere. Other links show their text, and images show their alt text, so nothing loads.
  const components = {
    a: ({ href, children }: { href?: string; children?: ReactNode }) => {
      const tag = TAG_HREF.exec(href ?? '');
      if (!tag) return <>{children}</>;
      const label = `[M${tag[1]}]`;
      const meeting = links[Number(tag[1]) - 1];
      if (!meeting) return <>{label}</>;
      if (!meeting.id) return <span title={meeting.title}>{label}</span>;
      return (
        <Link href={meetingHref({ id: meeting.id })} title={meeting.title} className="hover:underline">
          {label}
        </Link>
      );
    },
    img: ({ alt }: { alt?: string }) => <>{alt}</>,
  };

  return (
    <article className="space-y-4 rounded-lg border border-border bg-card p-6">
      <header className="space-y-1">
        <h2 className="text-2xl font-semibold">Weekly digest</h2>
        <p className="text-sm text-muted-foreground">
          {formatDay(digest.periodStart)} – {formatDay(digest.periodEnd)}
        </p>
      </header>
      <div className="space-y-2 [&_h2]:mt-4 [&_h2]:text-lg [&_h2]:font-semibold [&_ul]:list-disc [&_ul]:pl-5">
        <ReactMarkdown skipHtml components={components}>{withTagLinks(digest.markdown)}</ReactMarkdown>
      </div>
    </article>
  );
}
