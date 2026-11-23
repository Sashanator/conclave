'use client';

import { useEffect, useState, type ReactNode } from 'react';
import Link from 'next/link';
import { invoke } from '@tauri-apps/api/core';
import ReactMarkdown from 'react-markdown';
import { meetingHref } from '@/lib/meeting-target';
import { formatLongDate } from '@/lib/dates';
import { rememberOrigin, type Origin } from '@/lib/back-nav';

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

// A YYYY-MM-DD is a local day; any other date string parses as is. Shown raw when it is not a date.
function formatDay(value: string): string {
  const d = new Date(/^\d{4}-\d{2}-\d{2}$/.test(value) ? `${value}T00:00` : value);
  return Number.isNaN(d.getTime()) ? value : formatLongDate(d);
}

export type DigestState = 'loading' | 'ready' | 'missing' | 'error';

export function useDigest(id: string | null): { state: DigestState; digest: DigestContent | null } {
  const [digest, setDigest] = useState<DigestContent | null>(null);
  const [state, setState] = useState<DigestState>('loading');

  useEffect(() => {
    if (id === null) {
      setDigest(null);
      setState('missing');
      return;
    }
    let live = true;
    setDigest(null);
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
        if (live) {
          setDigest(null);
          setState('error');
        }
      });
    return () => {
      live = false;
    };
  }, [id]);

  return { state, digest };
}

const CHIP = 'inline-block max-w-[28ch] truncate rounded border px-2 py-0.5 align-middle text-xs';
const CHIP_LINK = `${CHIP} border-primary/35 bg-primary/10 text-foreground hover:bg-primary/20`;

/** `fill` lets the chip grow to its container (the rail); the default stays compact for inline use. */
export function MeetingChip({ meeting, from, fill }: { meeting: { id: string; title: string }; from: Origin; fill?: boolean }) {
  const wide = fill ? ' !block !max-w-full' : '';
  if (!meeting.id) return <span title={meeting.title} className={`${CHIP} border-border text-muted-foreground${wide}`}>{meeting.title}</span>;
  return (
    <Link href={meetingHref({ id: meeting.id })} title={meeting.title} onClick={() => rememberOrigin(from)} className={CHIP_LINK + wide}>
      {meeting.title}
    </Link>
  );
}

/** `from` is where a chip's meeting page leads Back to. */
export function DigestView({ digest, from }: { digest: DigestContent; from: Origin }) {
  const links = digest.meetingLinks;
  // Only our [Mn] tags link anywhere. Other links show their text, and images show their alt text, so nothing loads.
  const components = {
    a: ({ href, children }: { href?: string; children?: ReactNode }) => {
      const tag = TAG_HREF.exec(href ?? '');
      if (!tag) return <>{children}</>;
      const label = `[M${tag[1]}]`;
      const meeting = links[Number(tag[1]) - 1];
      if (!meeting) return <>{label}</>;
      return <MeetingChip meeting={meeting} from={from} />;
    },
    img: ({ alt }: { alt?: string }) => <>{alt}</>,
  };

  return (
    <article className="space-y-4">
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
