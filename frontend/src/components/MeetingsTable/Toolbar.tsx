'use client';

import { useState, type RefObject } from 'react';
import { addDays, format } from 'date-fns';
import { ArrowDown, ArrowUp, ChevronDown, MessageCircleQuestion, Plus, Search, X } from 'lucide-react';
import {
  DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel,
  DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuSeparator, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import type { MeetingState } from '@/lib/meetings';
import { COLUMNS, effectiveHidden, weekRange, type CategoryInfo, type SortId, type ViewAction, type ViewConfig, type ViewFilter, type ViewGroup } from '@/lib/meeting-views';

type Dispatch = (a: ViewAction) => void;
const btn = 'inline-flex h-[30px] items-center gap-1.5 rounded-md border border-transparent px-2.5 text-sm text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring';
const STATE_FILTERS: [MeetingState, string][] = [['recording', 'Recording'], ['finished', 'Finished'], ['cancelled', 'Cancelled'], ['not-recorded', 'Not recorded'], ['new', 'New']];
const kbd = 'rounded border px-1 text-[11px] text-muted-foreground';

export const SORTABLE: SortId[] = ['name', 'date', 'category', 'reviewed'];
export const TITLES: Record<string, string> = { name: 'Name', date: 'Date', category: 'Category', summary: 'Summary', reviewed: 'Reviewed' };
const GROUP_TITLES: Record<ViewGroup, string> = { none: 'None', day: 'Day', category: 'Category' };

export function Toolbar({ onNew, query, onQuery, searchRef, config, dispatch, onAsk }: {
  onNew: () => void;
  query: string;
  onQuery: (q: string) => void;
  searchRef: RefObject<HTMLInputElement>;
  config: ViewConfig;
  dispatch: Dispatch;
  onAsk?: () => void;
}) {
  const sort = config.sort[0];
  return (
    <div className="mb-2 flex flex-wrap items-center gap-1.5">
      <button type="button" onClick={onNew} title="New meeting (Ctrl N)"
        className="inline-flex h-[30px] items-center gap-1.5 rounded-md bg-primary px-2.5 text-sm font-semibold text-primary-foreground hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
        <Plus className="h-3.5 w-3.5" aria-hidden />New meeting <span className="rounded border border-primary-foreground/40 px-1 text-[11px]">Ctrl N</span>
      </button>
      <label className="relative min-w-[240px] max-w-[360px] flex-1">
        <Search className="pointer-events-none absolute left-2 top-2 h-3.5 w-3.5 text-muted-foreground" aria-hidden />
        <input ref={searchRef} value={query} onChange={(e) => onQuery(e.target.value)} aria-label="Search meetings, summaries and transcripts"
          placeholder="Search everything  ( / )"
          className="h-[30px] w-full rounded-md border bg-card pl-[30px] pr-7 text-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" />
        {query && (
          <button type="button" aria-label="Clear search" onClick={() => onQuery('')} className="absolute right-1.5 top-1.5 rounded text-muted-foreground hover:text-foreground">
            <X className="h-3.5 w-3.5" aria-hidden />
          </button>
        )}
      </label>
      <DropdownMenu>
        <DropdownMenuTrigger asChild><button type="button" className={btn}>Sort</button></DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          <DropdownMenuRadioGroup value={sort?.id} onValueChange={(id) => dispatch({ type: 'setSort', sort: [{ id: id as SortId, desc: sort?.desc ?? false }] })}>
            {SORTABLE.map((id) => <DropdownMenuRadioItem key={id} value={id}>{TITLES[id]}</DropdownMenuRadioItem>)}
          </DropdownMenuRadioGroup>
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={() => sort && dispatch({ type: 'setSort', sort: [{ ...sort, desc: false }] })}>Ascending</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => sort && dispatch({ type: 'setSort', sort: [{ ...sort, desc: true }] })}>Descending</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button type="button" className={`${btn} ${config.group !== 'none' ? 'bg-accent !text-foreground' : ''}`}>Group: {GROUP_TITLES[config.group]}</button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          <DropdownMenuRadioGroup value={config.group} onValueChange={(g) => dispatch({ type: 'setGroup', group: g as ViewGroup })}>
            {(Object.keys(GROUP_TITLES) as ViewGroup[]).map((g) => <DropdownMenuRadioItem key={g} value={g}>{GROUP_TITLES[g]}</DropdownMenuRadioItem>)}
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
      <span className="flex-1" />
      <button type="button" disabled={!onAsk} onClick={onAsk} title={onAsk ? 'Ask (Ctrl J)' : 'Ask is not available yet'}
        className={`${btn} disabled:cursor-not-allowed disabled:opacity-50`}>
        <MessageCircleQuestion className="h-3.5 w-3.5" aria-hidden />Ask <span className={kbd}>Ctrl J</span>
      </button>
    </div>
  );
}

function chipsOf(f: ViewFilter, now: Date, templates: CategoryInfo[]): { key: keyof ViewFilter; label: string }[] {
  const out: { key: keyof ViewFilter; label: string }[] = [];
  if (f.range) {
    const { from, to } = weekRange(now);
    out.push({ key: 'range', label: f.range === 'this-week'
      ? `Date: This week (${format(from, 'EEE d')} – ${format(addDays(to, -1), 'EEE d MMM')})`
      : `Date: ${f.range.from} – ${f.range.to}` });
  }
  if (f.reviewed !== undefined) out.push({ key: 'reviewed', label: `Reviewed: ${f.reviewed ? 'Yes' : 'No'}` });
  if (f.categories) {
    const name = (c: string | null) => (c === null ? 'No template' : templates.find((t) => t.id === c)?.name ?? c);
    out.push({ key: 'categories', label: `Category: ${f.categories.map(name).join(', ') || 'none'}` });
  }
  if (f.states) out.push({ key: 'states', label: `Status: ${f.states.join(', ') || 'none'}` });
  if (f.calendar !== undefined) out.push({ key: 'calendar', label: `Calendar: ${f.calendar ? 'Yes' : 'No'}` });
  return out;
}

export function FilterChips({ filter, now, templates, dispatch }: { filter: ViewFilter; now: Date; templates: CategoryInfo[]; dispatch: Dispatch }) {
  const set = (next: ViewFilter) => dispatch({ type: 'setFilter', filter: next });
  const cats = filter.categories ?? [];
  const states = filter.states ?? [];
  const [custom, setCustom] = useState(false);
  const toggleCat = (c: string | null) => set({ ...filter, categories: cats.includes(c) ? cats.filter((x) => x !== c) : [...cats, c] });
  return (
    <div className="mb-2 flex flex-wrap items-center gap-1.5 text-xs">
      {chipsOf(filter, now, templates).map(({ key, label }) => (
        <span key={key} className="inline-flex items-center gap-1 rounded-full border px-2.5 py-0.5 text-foreground">
          {label}
          <button type="button" aria-label={`Remove filter: ${label}`} onClick={() => { const { [key]: _drop, ...rest } = filter; set(rest); }}>×</button>
        </span>
      ))}
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button type="button" className="rounded-full border border-dashed px-2.5 py-0.5 text-muted-foreground hover:text-foreground">+ Filter</button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          <DropdownMenuItem onSelect={() => set({ ...filter, range: 'this-week' })}>Date: This week</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => set({ ...filter, reviewed: false })}>Reviewed: No</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => set({ ...filter, reviewed: true })}>Reviewed: Yes</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => set({ ...filter, calendar: true })}>Calendar: Yes</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => set({ ...filter, calendar: false })}>Calendar: No</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => setCustom(true)}>Date: Custom range…</DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuLabel>Status</DropdownMenuLabel>
          {STATE_FILTERS.map(([s, name]) => (
            <DropdownMenuCheckboxItem key={s} checked={states.includes(s)} onCheckedChange={() => { const next = states.includes(s) ? states.filter((x) => x !== s) : [...states, s]; set({ ...filter, states: next.length ? next : undefined }); }}>{name}</DropdownMenuCheckboxItem>
          ))}
          <DropdownMenuSeparator />
          <DropdownMenuLabel>Category</DropdownMenuLabel>
          {[...templates.map((t) => ({ id: t.id as string | null, name: t.name })), { id: null, name: 'No template' }].map((c) => (
            <DropdownMenuCheckboxItem key={c.id ?? 'none'} checked={cats.includes(c.id)} onCheckedChange={() => toggleCat(c.id)}>{c.name}</DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
      {custom && (
        <form className="flex items-center gap-1" onSubmit={(e) => {
          e.preventDefault();
          const d = new FormData(e.currentTarget);
          const from = String(d.get('from') ?? ''), to = String(d.get('to') ?? '');
          if (!from || !to) return;
          set({ ...filter, range: from <= to ? { from, to } : { from: to, to: from } });
          setCustom(false);
        }}>
          <input type="date" name="from" required aria-label="From date" className="h-7 rounded-md border bg-card px-1.5" />
          <span aria-hidden>–</span>
          <input type="date" name="to" required aria-label="To date" className="h-7 rounded-md border bg-card px-1.5" />
          <button type="submit" className="h-7 rounded-md bg-primary px-2 font-semibold text-primary-foreground">Apply</button>
          <button type="button" onClick={() => setCustom(false)} className="h-7 rounded-md px-2 text-muted-foreground hover:text-foreground">Cancel</button>
        </form>
      )}
    </div>
  );
}

/** Click-to-sort target: the first click sorts ascending, the next flips it. */
export const nextSort = (config: ViewConfig, id: SortId): ViewAction =>
  ({ type: 'setSort', sort: [{ id, desc: config.sort[0]?.id === id ? !config.sort[0].desc : false }] });

export function SortIndicator({ config, id }: { config: ViewConfig; id: string }) {
  const s = config.sort[0];
  if (s?.id !== id) return null;
  return s.desc ? <ArrowDown className="ml-1 inline h-3 w-3" aria-hidden /> : <ArrowUp className="ml-1 inline h-3 w-3" aria-hidden />;
}

export function HeaderMenu({ id, config, dispatch }: { id: string; config: ViewConfig; dispatch: Dispatch }) {
  const hidden = effectiveHidden(config);
  const sortable = SORTABLE.includes(id as SortId);
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button type="button" aria-label={`${TITLES[id]} column menu`} className="ml-0.5 rounded text-muted-foreground hover:text-foreground">
          <ChevronDown className="h-3 w-3" aria-hidden />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        {sortable && <DropdownMenuItem onSelect={() => dispatch({ type: 'setSort', sort: [{ id: id as SortId, desc: false }] })}>Sort ascending</DropdownMenuItem>}
        {sortable && <DropdownMenuItem onSelect={() => dispatch({ type: 'setSort', sort: [{ id: id as SortId, desc: true }] })}>Sort descending</DropdownMenuItem>}
        {id !== 'name' && <DropdownMenuItem onSelect={() => dispatch({ type: 'toggleHidden', column: id })}>Hide column</DropdownMenuItem>}
        <DropdownMenuSeparator />
        <DropdownMenuLabel>Columns</DropdownMenuLabel>
        {COLUMNS.map((c) => (
          <DropdownMenuCheckboxItem key={c} checked={!hidden.includes(c)} disabled={c === 'name' || (c === 'category' && config.group === 'category')}
            onCheckedChange={() => dispatch({ type: 'toggleHidden', column: c })}>{TITLES[c]}</DropdownMenuCheckboxItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
