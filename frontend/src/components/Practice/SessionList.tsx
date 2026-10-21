import { Button } from '@/components/ui/button';
import { minutesLabel, type SessionSummary } from '@/lib/practice';

// "Oct 19 · 18 min · 4 grammar · 31 fillers"
function sessionLine(s: SessionSummary): string {
  const day = new Date(s.started_at).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  const parts = [day, minutesLabel(s.duration_secs)];
  if (s.summary) parts.push(s.summary);
  return parts.join(' · ');
}

export function SessionList({ sessions, onOpen, onDelete }: {
  sessions: SessionSummary[];
  onOpen: (session: SessionSummary) => void;
  onDelete: (session: SessionSummary) => void;
}) {
  return (
    <section className="bg-card rounded-lg border border-border p-6 shadow-sm">
      <h3 className="text-lg font-semibold text-foreground mb-2">Past sessions</h3>

      <ul className="divide-y divide-border">
        {sessions.map((s) => (
          <li key={s.id} className="flex flex-wrap items-center gap-3 py-3">
            <div className="flex-1 min-w-0">
              <div className="font-medium truncate">{s.setup_name}</div>
              <p className="text-sm text-muted-foreground">{sessionLine(s)}</p>
            </div>
            <Button variant="outline" size="sm" onClick={() => onOpen(s)}>Open report</Button>
            <Button variant="destructive" size="sm" onClick={() => onDelete(s)}>Delete</Button>
          </li>
        ))}
      </ul>
    </section>
  );
}
