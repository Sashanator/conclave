import { Button } from '@/components/ui/button';

const RUNNING = ['processing', 'summarizing', 'regenerating'];

/** The note's text and target, or null when the displayed summary needs none. */
export function templateMismatch(p: {
  shown: { id: string; name: string } | null;
  currentId: string | null;
  templates: Array<{ id: string; name: string }>;
  status: string;
  autoPending: boolean;
}) {
  if (!p.shown || !p.currentId || p.shown.id === p.currentId) return null;
  if (RUNNING.includes(p.status) || p.autoPending) return null;
  const current = p.templates.find((t) => t.id === p.currentId);
  return current ? { shown: p.shown.name, current } : null;
}

/** Persistent hint: the summary was made with another template than the meeting's category. Regenerating is the user's click. */
export function TemplateMismatchNote({ mismatch, onRegenerate }: {
  mismatch: NonNullable<ReturnType<typeof templateMismatch>>;
  onRegenerate: (templateId: string) => void;
}) {
  return (
    <div role="status" className="mb-3 flex flex-wrap items-center gap-2 rounded-md border border-border bg-muted/40 px-3 py-2 text-sm">
      <span>Made with {mismatch.shown}. This meeting is now {mismatch.current.name}.</span>
      <Button size="sm" variant="outline" onClick={() => onRegenerate(mismatch.current.id)}>Regenerate with {mismatch.current.name}</Button>
    </div>
  );
}
