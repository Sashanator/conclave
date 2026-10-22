import { hitTarget, meetingHref, type SearchHit } from '@/lib/meeting-target';

const SOURCE_LABELS: Record<SearchHit['source'], string> = {
  title: 'Title',
  transcript: 'Transcript',
  summary: 'Summary',
  notes: 'Notes',
};

// Hit rows under a matched meeting. The snippet is plain text, never markup.
export function SearchHits({ hits, onOpen }: { hits: SearchHit[]; onOpen: (href: string) => void }) {
  return (
    <div className="mt-1 ml-8 space-y-1">
      {hits.map((hit, i) => (
        <button
          key={`${hit.source}-${hit.transcriptId ?? i}`}
          type="button"
          // The row around the hit must not run too: it would drop the line and tab
          onClick={(e) => {
            e.stopPropagation();
            onOpen(meetingHref(hitTarget(hit)));
          }}
          className="block w-full text-left text-xs text-muted-foreground bg-yellow-50 dark:bg-yellow-950/40 p-1.5 rounded border border-yellow-100 dark:border-yellow-800 hover:bg-yellow-100 dark:hover:bg-yellow-900/40"
        >
          <span className="font-medium text-yellow-600 mr-1">{SOURCE_LABELS[hit.source]}:</span>
          <span className="line-clamp-2">{hit.snippet}</span>
        </button>
      ))}
    </div>
  );
}
