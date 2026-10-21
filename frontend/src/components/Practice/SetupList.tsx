import { Button } from '@/components/ui/button';
import type { SetupSummary } from '@/lib/practice';

export function SetupList({ setups, canStart, onNew, onEdit, onStart }: {
  setups: SetupSummary[];
  canStart: boolean;
  onNew: () => void;
  onEdit: (setup: SetupSummary) => void;
  onStart: (setup: SetupSummary) => void;
}) {
  return (
    <section className="bg-card rounded-lg border border-border p-6 shadow-sm">
      <div className="flex items-center justify-between mb-2">
        <h3 className="text-lg font-semibold text-foreground">Your setups</h3>
        <Button variant="outline" size="sm" onClick={onNew}>+ New setup</Button>
      </div>

      <ul className="divide-y divide-border">
        {setups.map((s) => (
          <li key={s.id} className="flex flex-wrap items-center gap-3 py-3">
            <div className="flex-1 min-w-0">
              <div>
                <span className="font-medium">{s.name}</span>
                {s.seeded && <span className="ml-2 rounded px-1.5 py-0.5 text-xs bg-muted text-muted-foreground">example</span>}
                {s.materials.map((m) => (
                  <span key={m.id} className="ml-2 rounded px-1.5 py-0.5 text-xs bg-muted text-muted-foreground">{m.name}</span>
                ))}
              </div>
              <p className="text-sm text-muted-foreground">
                {`${s.character} · ${s.session_count === 1 ? '1 session' : `${s.session_count} sessions`}`}
              </p>
            </div>
            <Button variant="outline" size="sm" onClick={() => onEdit(s)}>Edit</Button>
            <Button size="sm" disabled={!canStart} onClick={() => onStart(s)}>▶ Start</Button>
          </li>
        ))}
      </ul>
    </section>
  );
}
