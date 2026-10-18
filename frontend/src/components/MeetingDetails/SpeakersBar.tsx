'use client';
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Command, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command';
import { Button } from '@/components/ui/button';
import { formatTalkTime, speakerChips, type MeetingSpeakers } from '@/lib/speakers';
import type { Person } from '@/types';

const DOTS = ['bg-blue-500', 'bg-emerald-500', 'bg-amber-500', 'bg-violet-500', 'bg-rose-500', 'bg-cyan-500'];
const NAME_MAX = 80;
type NameTarget = { personId: string } | { newName: string };

interface SpeakersBarProps {
  data: MeetingSpeakers | null;
  busy: boolean;
  percent: number | null;
  onName: (idx: number, target: NameTarget) => Promise<string | null>;
  onIdentify: () => Promise<void>;
}

// Above the transcript: progress, failure and skip notes, and one chip per speaker
export function SpeakersBar({ data, busy, percent, onName, onIdentify }: SpeakersBarProps) {
  const [people, setPeople] = useState<Person[]>([]);
  // Reloaded with each speaker reload, so a new person shows up in the suggestions
  useEffect(() => {
    invoke<Person[]>('people_list')
      .then(setPeople)
      .catch((err) => console.error('Failed to load known voices:', err));
  }, [data]);

  const job = data?.job ?? null;
  const chips = speakerChips(data?.speakers ?? []);
  const failed = job?.status === 'failed';
  const skipped = job?.status === 'skipped' && job.reason === 'no_recording';
  if (!busy && chips.length === 0 && !failed && !skipped) return null;

  return (
    <div className="px-4 py-2 border-b border-border flex flex-col gap-2">
      {busy && <p className="text-sm text-muted-foreground">{`Identifying speakers… ${percent ?? 0}%`}</p>}
      {failed && (
        <p className="text-sm text-red-700 dark:text-red-300">
          {`Speaker analysis failed: ${job.reason}. `}
          <button type="button" className="underline" onClick={() => void onIdentify()}>Retry</button>
        </p>
      )}
      {skipped && <p className="text-xs text-muted-foreground">No recording saved, so speakers weren&apos;t identified</p>}
      {chips.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {chips.map((chip, i) => (
            <SpeakerChip
              key={chip.key}
              name={chip.name}
              seconds={chip.seconds}
              idxs={chip.idxs}
              isSelf={chip.isSelf}
              dot={DOTS[i % DOTS.length]}
              named={data?.speakers.find((s) => s.idx === chip.idxs[0])?.person_id != null}
              people={people}
              disabled={busy}
              onName={onName}
            />
          ))}
        </div>
      )}
    </div>
  );
}

interface SpeakerChipProps {
  name: string;
  seconds: number;
  idxs: number[];
  isSelf: boolean;
  dot: string;
  named: boolean;
  people: Person[];
  disabled: boolean;
  onName: (idx: number, target: NameTarget) => Promise<string | null>;
}

function SpeakerChip({ name, seconds, idxs, isSelf, dot, named, people, disabled, onName }: SpeakerChipProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  // A job that starts while the box is open closes it
  useEffect(() => { if (disabled) setOpen(false); }, [disabled]);

  const body = (
    <>
      <span className={`h-2 w-2 rounded-full ${dot}`} aria-hidden="true" />
      <span>{name}</span>
      <span className="text-xs text-muted-foreground">{formatTalkTime(seconds)}</span>
      {!named && <span aria-hidden="true">✎</span>}
    </>
  );

  // Renaming yourself happens in Settings; the backend refuses idx 0
  if (isSelf) {
    return <span data-chip={name} className="inline-flex h-7 items-center gap-1.5 px-2 text-sm">{body}</span>;
  }

  const pick = async (target: NameTarget) => {
    setOpen(false);
    setQuery('');
    const [first, ...rest] = idxs.filter((idx) => idx !== 0);
    if (first === undefined) return;
    // The first name creates or finds the person; the rest join that person by id
    const personId = await onName(first, target);
    if (personId == null) return;
    for (const idx of rest) await onName(idx, { personId });
  };

  const typed = query.trim();

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button variant="outline" size="sm" className="h-7 gap-1.5 px-2" disabled={disabled} data-chip={name}>
          {body}
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-64 p-0" align="start">
        <Command>
          <CommandInput placeholder="Name…" value={query} onValueChange={setQuery} />
          <CommandList>
            <CommandGroup>
              {people.map((p) => (
                <CommandItem key={p.id} value={p.name} onSelect={() => void pick({ personId: p.id })}>
                  {p.name}
                </CommandItem>
              ))}
              {/* forceMount: the name being typed would otherwise filter this item out */}
              <CommandItem
                forceMount
                value="New person…"
                disabled={!typed || typed.length > NAME_MAX}
                onSelect={() => void pick({ newName: typed })}
              >
                New person…
              </CommandItem>
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
