'use client';

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { X } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { LlmOverridePicker } from '@/components/shared/LlmOverridePicker';

// The Rust `KnowledgeSettingsView` (snake_case, no rename). The calendar URL never reaches the frontend, only its host.
type KnowledgeView = {
  provider: string | null;
  model: string | null;
  my_names: string[];
  extract_enabled: boolean;
  digest_enabled: boolean;
  calendar_host: string | null;
};

// Same limits as the backend (knowledge/mod.rs)
const MAX_NAMES = 20;
const MAX_NAME_CHARS = 40;

function toInput(v: KnowledgeView) {
  return {
    provider: v.provider,
    model: v.model,
    my_names: v.my_names,
    extract_enabled: v.extract_enabled,
    digest_enabled: v.digest_enabled,
  };
}

export function KnowledgeSettings() {
  const [view, setView] = useState<KnowledgeView | null>(null);
  const [name, setName] = useState('');
  // A provider picked without a model. Shown but not saved: the backend takes a provider and model together or neither.
  const [pendingProvider, setPendingProvider] = useState<string | null>(null);
  // Edits show at once. Saves run one after another, each from the latest edit.
  const latest = useRef<KnowledgeView | null>(null);
  const queue = useRef<Promise<void>>(Promise.resolve());
  // Bumped after a failed save and its reload: saves queued before that are dropped
  const generation = useRef(0);

  const show = (v: KnowledgeView) => {
    latest.current = v;
    setView(v);
  };

  const load = async () => {
    try {
      show(await invoke<KnowledgeView>('knowledge_get_settings'));
    } catch (err) {
      toast.error('Could not load Knowledge settings', { description: String(err) });
    }
  };

  useEffect(() => {
    load();
  }, []);

  const commit = (change: (v: KnowledgeView) => KnowledgeView) => {
    if (!latest.current) return;
    const next = change(latest.current);
    const gen = generation.current;
    show(next);
    queue.current = queue.current.then(async () => {
      if (gen !== generation.current) return;
      try {
        const saved = await invoke<KnowledgeView>('knowledge_save_settings', { input: toInput(next) });
        // A newer edit has already replaced this one, so only the newest result is shown
        if (latest.current === next) show(saved);
      } catch (err) {
        toast.error('Knowledge settings were not saved', { description: String(err) });
        await load();
        generation.current++;
      }
    });
  };

  if (!view) return null;

  const addName = () => {
    const text = name.trim();
    const taken = view.my_names.some((n) => n.toLowerCase() === text.toLowerCase());
    if (!text || taken || [...text].length > MAX_NAME_CHARS || view.my_names.length >= MAX_NAMES) return; // rejected: keep the text so it can be fixed
    commit((v) => ({ ...v, my_names: [...v.my_names, text] }));
    setName('');
  };

  return (
    <div className="space-y-6">
      <section className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-5">
        <h3 className="text-lg font-semibold text-foreground">Knowledge</h3>

        <LlmOverridePicker
          idPrefix="knowledge"
          providerLabel="Knowledge provider"
          modelLabel="Knowledge model"
          provider={pendingProvider ?? view.provider}
          model={pendingProvider ? null : view.model}
          onChange={(provider, model) => {
            if (provider && !model) {
              setPendingProvider(provider);
              return;
            }
            setPendingProvider(null);
            commit((v) => ({ ...v, provider, model }));
          }}
        />

        <div className="flex items-center justify-between gap-4">
          <Label htmlFor="knowledge-extract">Extract my tasks after each summary</Label>
          <Switch
            id="knowledge-extract"
            aria-label="Extract my tasks after each summary"
            checked={view.extract_enabled}
            onCheckedChange={(on) => commit((v) => ({ ...v, extract_enabled: on }))}
          />
        </div>

        <div className="flex items-center justify-between gap-4">
          <Label htmlFor="knowledge-digest">Weekly digest</Label>
          <Switch
            id="knowledge-digest"
            aria-label="Weekly digest"
            checked={view.digest_enabled}
            onCheckedChange={(on) => commit((v) => ({ ...v, digest_enabled: on }))}
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="knowledge-name">My names</Label>
          <Input
            id="knowledge-name"
            aria-label="Add a name"
            placeholder="Add a name and press Enter"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                addName();
              }
            }}
          />

          <ul className="flex flex-wrap gap-2">
            {view.my_names.map((n) => (
              <li key={n} className="flex items-center gap-1.5 rounded-full border border-border px-3 py-1 text-sm">
                {n}
                <button
                  type="button"
                  aria-label={`Remove ${n}`}
                  className="text-muted-foreground hover:text-foreground"
                  onClick={() => commit((v) => ({ ...v, my_names: v.my_names.filter((x) => x !== n) }))}
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              </li>
            ))}
          </ul>

          <p className="text-xs text-muted-foreground">
            Names the summaries use for you, e.g. Sasha, Саша, Alexander
          </p>
        </div>
      </section>

      <section className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-2">
        <h3 className="text-lg font-semibold text-foreground">Calendar</h3>
        <p className="text-sm text-muted-foreground">{view.calendar_host ?? 'Not connected'}</p>
      </section>
    </div>
  );
}
