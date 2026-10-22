'use client';

import { Fragment, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Plus, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { PROVIDERS, configuredProviders } from '@/components/shared/LlmOverridePicker';

// Mirrors usage::prices::PriceRow. Prices are dollars per million tokens.
interface PriceRow {
  provider: string;
  model: string;
  input: number;
  output: number;
  cache_read: number;
  cache_write: number;
  built_in: boolean;
}

type Field = 'input' | 'output' | 'cache_read' | 'cache_write';
const FIELDS: { field: Field; label: string }[] = [
  { field: 'input', label: 'Input' },
  { field: 'output', label: 'Output' },
  { field: 'cache_read', label: 'Cache read' },
  { field: 'cache_write', label: 'Cache write' },
];

interface NewRow {
  provider: string;
  model: string;
  prices: Record<Field, string>;
}
const EMPTY_NEW: NewRow = { provider: '', model: '', prices: { input: '', output: '', cache_read: '', cache_write: '' } };

const NEW = 'new';
// Errors are keyed like drafts: one per price field, plus one for the row itself (a failed delete)
const ROW = 'row';
const rowKey = (r: Pick<PriceRow, 'provider' | 'model'>) => JSON.stringify([r.provider, r.model]);
const draftKey = (key: string, field: Field | typeof ROW) => `${key}|${field}`;
const providerLabel = (id: string) => PROVIDERS.find((p) => p.id === id)?.label ?? id;

function parsePrice(text: string): number | null {
  const t = text.trim();
  if (t === '') return null;
  const n = Number(t);
  return Number.isFinite(n) ? n : null;
}

function without<T>(record: Record<string, T>, key: string): Record<string, T> {
  const next = { ...record };
  delete next[key];
  return next;
}

export function CostsSettings() {
  const [rows, setRows] = useState<PriceRow[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [draft, setDraft] = useState<NewRow | null>(null);
  const [providers, setProviders] = useState<{ id: string; label: string }[]>([]);
  // The backend replaces the whole override set, so every save sends the full list. Saves run one at a
  // time, each built from the latest list, so quick successive blurs cannot overwrite each other.
  const latest = useRef<PriceRow[] | null>(null);
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  // Set synchronously, so a second blur while the first add is saving cannot append the row twice
  const addInFlight = useRef(false);

  useEffect(() => {
    invoke<PriceRow[]>('costs_get_prices')
      .then(apply)
      .catch((err) => setLoadError(String(err)));
  }, []);

  function apply(list: PriceRow[]) {
    latest.current = list;
    setRows(list);
  }

  // errorKey is where a rejection is shown; a success clears only that error, so other edits keep theirs
  function commit(errorKey: string, change: (current: PriceRow[]) => PriceRow[]): Promise<boolean> {
    const run = queue.current.then(async () => {
      // Never saved before the list loaded: a save would replace every saved price with this one change
      if (!latest.current) return false;
      try {
        const saved = await invoke<PriceRow[]>('costs_save_prices', { rows: change(latest.current) });
        apply(saved);
        setErrors((e) => without(e, errorKey));
        return true;
      } catch (err) {
        setErrors((e) => ({ ...e, [errorKey]: String(err) }));
        return false;
      }
    });
    queue.current = run;
    return run;
  }

  function priceBlur(row: PriceRow, field: Field) {
    const key = rowKey(row);
    const dk = draftKey(key, field);
    const text = drafts[dk];
    if (text === undefined) return;
    const value = parsePrice(text);
    if (value === null) {
      setErrors((e) => ({ ...e, [dk]: 'Enter a number' }));
      return;
    }
    if (value === row[field]) {
      setDrafts((d) => without(d, dk));
      setErrors((e) => without(e, dk));
      return;
    }
    void commit(dk, (current) => current.map((r) => (rowKey(r) === key ? { ...r, [field]: value } : r))).then((ok) => {
      if (ok) setDrafts((d) => without(d, dk));
    });
  }

  function remove(row: PriceRow) {
    const key = rowKey(row);
    void commit(draftKey(key, ROW), (current) => current.filter((r) => rowKey(r) !== key));
  }

  function openAdd() {
    setErrors((e) => without(e, NEW));
    setDraft(EMPTY_NEW);
    configuredProviders().then((found) => setProviders(PROVIDERS.filter((p) => found.has(p.id))));
  }

  // Saves once the provider, model and all four prices are filled in; until then it waits silently
  function tryAdd(next: NewRow) {
    const prices = FIELDS.map(({ field }) => parsePrice(next.prices[field]));
    if (addInFlight.current || !next.provider || !next.model.trim() || prices.some((p) => p === null)) return;
    const [input, output, cache_read, cache_write] = prices as number[];
    const row: PriceRow = { provider: next.provider, model: next.model.trim(), input, output, cache_read, cache_write, built_in: false };
    addInFlight.current = true;
    void commit(NEW, (current) => [...current, row]).then((ok) => {
      addInFlight.current = false;
      if (ok) setDraft(null);
    });
  }

  function cancelAdd() {
    setDraft(null);
  }

  // Each error sits under its own row, so an error for one field does not hide another field's error
  const messagesFor = (key: string) =>
    [...FIELDS.map(({ field }) => errors[draftKey(key, field)]), errors[draftKey(key, ROW)]].filter((m): m is string => Boolean(m));

  if (loadError) return <p role="alert" className="text-red-600">{`Could not load model prices: ${loadError}`}</p>;
  if (!rows) return <p className="text-muted-foreground">Loading…</p>;

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-lg font-semibold">Model prices</h2>
        <p className="text-sm text-muted-foreground">Dollars per million tokens, used to estimate the cost of each LLM call.</p>
      </div>

      <table className="w-full text-sm">
        <thead>
          <tr className="text-left text-muted-foreground">
            <th className="py-2 font-medium">Provider</th>
            <th className="py-2 font-medium">Model</th>
            {FIELDS.map(({ field, label }) => (
              <th key={field} className="py-2 font-medium">{label}</th>
            ))}
            <th className="py-2"><span className="sr-only">Actions</span></th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => {
            const key = rowKey(row);
            return (
              <Fragment key={key}>
                <tr className="border-t border-border">
                  <td className="py-2">{providerLabel(row.provider)}</td>
                  <td className="py-2">{row.model}</td>
                  {FIELDS.map(({ field, label }) => {
                    const dk = draftKey(key, field);
                    return (
                      <td key={field} className="py-2 pr-2">
                        <Input
                          type="number"
                          step="any"
                          aria-label={`${label} price for ${row.provider} ${row.model}`}
                          className="h-8 w-24"
                          value={drafts[dk] ?? String(row[field])}
                          onChange={(e) => setDrafts((d) => ({ ...d, [dk]: e.target.value }))}
                          onBlur={() => priceBlur(row, field)}
                        />
                      </td>
                    );
                  })}
                  <td className="py-2">
                    {!row.built_in && (
                      <Button variant="ghost" size="icon" aria-label={`Delete ${row.provider} ${row.model}`} onClick={() => remove(row)}>
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    )}
                  </td>
                </tr>
                {messagesFor(key).map((message, i) => (
                  <tr key={i}>
                    <td colSpan={FIELDS.length + 3} className="pb-2 text-red-600">{message}</td>
                  </tr>
                ))}
              </Fragment>
            );
          })}
        </tbody>
      </table>

      {draft ? (
        <div className="space-y-2 rounded-lg border border-border p-3">
          <div className="flex flex-wrap items-center gap-2">
            <Select
              value={draft.provider}
              onValueChange={(provider) => {
                const next = { ...draft, provider };
                setDraft(next);
                tryAdd(next);
              }}
            >
              <SelectTrigger aria-label="Provider for the new model" className="w-44">
                <SelectValue placeholder="Provider" />
              </SelectTrigger>
              <SelectContent>
                {providers.map((p) => (
                  <SelectItem key={p.id} value={p.id}>{p.label}</SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Input
              aria-label="New model id"
              placeholder="Model"
              className="h-8 w-56"
              value={draft.model}
              onChange={(e) => setDraft({ ...draft, model: e.target.value })}
              onBlur={() => tryAdd(draft)}
            />
            {FIELDS.map(({ field, label }) => (
              <Input
                key={field}
                type="number"
                step="any"
                aria-label={`${label} price for new model`}
                placeholder={label}
                className="h-8 w-28"
                value={draft.prices[field]}
                onChange={(e) => setDraft({ ...draft, prices: { ...draft.prices, [field]: e.target.value } })}
                onBlur={() => tryAdd(draft)}
              />
            ))}
            <Button variant="ghost" size="sm" onClick={cancelAdd}>Cancel</Button>
          </div>
          {errors[NEW] && <p className="text-red-600">{errors[NEW]}</p>}
        </div>
      ) : (
        <Button variant="outline" onClick={openAdd}>
          <Plus className="mr-2 h-4 w-4" />
          Add a model
        </Button>
      )}
    </div>
  );
}
