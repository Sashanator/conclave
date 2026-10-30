'use client';

import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { Button } from '@/components/ui/button';
import type { TemplateJson } from '@/lib/template-draft';
import type { ChangeNote } from '@/lib/templateAssist';
import { collapseSame, diffItems, matchSections, mergeSelected } from '@/lib/templateDiff';

export function TemplateDiffReview(props: {
  old: TemplateJson; next: TemplateJson;
  changes?: ChangeNote[]; warnings?: string[];
  selectable: boolean; acceptLabel: string; notice?: string | null; busy?: boolean;
  onAccept: (merged: TemplateJson) => void; onDiscard: () => void;
  children?: ReactNode;
}): JSX.Element {
  const { old, next, changes, warnings, selectable, acceptLabel, notice, busy = false, onAccept, onDiscard, children } = props;
  const items = useMemo(() => diffItems(old, next), [old, next]);
  const [selected, setSelected] = useState<Set<string>>(() => new Set(items.map(i => i.key)));
  useEffect(() => { setSelected(new Set(items.map(i => i.key))); }, [items]);
  const unchanged = matchSections(old.sections, next.sections).filter(p => p.status === 'unchanged').length;

  return <div className="space-y-4 text-sm">
    {!!changes?.length && <div>
      <h3 className="font-medium">What I changed and why</h3>
      <ul className="list-disc pl-5">{changes.map((c, i) => <li key={i}>{c.field}: {c.note}</li>)}</ul>
    </div>}
    {!!warnings?.length && <ul className="list-disc pl-5 text-amber-700 dark:text-amber-400">
      {warnings.map((w, i) => <li key={i}>{w}</li>)}
    </ul>}
    {items.length === 0 && <p>No differences</p>}
    {items.map(item => <div key={item.key} className="rounded-md border border-border p-3 space-y-2">
      <label className="flex items-center gap-2 font-medium">
        {selectable && <input type="checkbox" aria-label={item.label} checked={selected.has(item.key)} onChange={e => {
          const checked = e.target.checked;
          setSelected(previous => {
            const updated = new Set(previous);
            if (checked) updated.add(item.key); else updated.delete(item.key);
            return updated;
          });
        }} />}
        {item.label}
      </label>
      <div className="whitespace-pre-wrap break-words">
        {item.kind === 'scalar' ? <>{item.old} → {item.next}</> : item.parts ? item.parts.map((part, i) =>
          part.kind === 'del' ? <del key={i} className="bg-red-100 text-red-900 dark:bg-red-950 dark:text-red-200">{part.text}</del>
            : part.kind === 'add' ? <ins key={i} className="bg-green-100 text-green-900 dark:bg-green-950 dark:text-green-200">{part.text}</ins>
              : <span key={i}>{collapseSame(part.text)}</span>,
        ) : <><span className="mr-2 rounded bg-muted px-2 py-0.5 text-xs">{item.status}</span>{item.status === 'removed' ? item.old : item.next}</>}
      </div>
    </div>)}
    <p className="text-muted-foreground">{unchanged} unchanged sections</p>
    {notice && <p role="status" className="text-amber-700 dark:text-amber-400">{notice}</p>}
    {children}
    <div className="flex justify-end gap-2">
      <Button variant="outline" disabled={busy} onClick={onDiscard}>Discard</Button>
      <Button disabled={busy || (selectable && selected.size === 0)} onClick={() => onAccept(selectable ? mergeSelected(old, next, selected) : next)}>{acceptLabel}</Button>
    </div>
  </div>;
}
