'use client';

import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { LlmOverridePicker } from '@/components/shared/LlmOverridePicker';
import { getAssistSettings, saveAssistSettings, type TemplateAssistSettings as AssistSettings } from '@/lib/templateAssist';

export function TemplateAssistSettings() {
  const [view, setView] = useState<AssistSettings | null>(null);
  // A provider picked without a model. Shown but not saved: the backend takes a provider and model together or neither.
  const [pendingProvider, setPendingProvider] = useState<string | null>(null);
  // Edits show at once. Saves run one after another, each from the latest edit.
  const latest = useRef<AssistSettings | null>(null);
  const queue = useRef<Promise<void>>(Promise.resolve());
  // Bumped after a failed save and its reload: saves queued before that are dropped
  const generation = useRef(0);

  const show = (v: AssistSettings) => {
    latest.current = v;
    setView(v);
  };

  const load = async () => {
    try {
      show(await getAssistSettings());
    } catch (err) {
      toast.error('Could not load template assistant settings', { description: String(err) });
    }
  };

  useEffect(() => {
    load();
  }, []);

  // The whole settings object is saved, so fields this card does not show (hideEditHint) are kept
  const commit = (change: (v: AssistSettings) => AssistSettings) => {
    if (!latest.current) return;
    const next = change(latest.current);
    const gen = generation.current;
    show(next);
    queue.current = queue.current.then(async () => {
      if (gen !== generation.current) return;
      try {
        await saveAssistSettings(next);
      } catch (err) {
        toast.error('Template assistant settings were not saved', { description: String(err) });
        await load();
        generation.current++;
      }
    });
  };

  if (!view) return null;

  return (
    <section className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-5">
      <h3 className="text-lg font-semibold text-foreground">Template assistant</h3>

      <LlmOverridePicker
        idPrefix="template-assist"
        providerLabel="Model for the template assistant"
        modelLabel="Model name"
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
        <Label htmlFor="template-assist-transcript">Include the transcript by default</Label>
        <Switch
          id="template-assist-transcript"
          aria-label="Include the transcript by default"
          checked={view.includeTranscript}
          onCheckedChange={(on) => commit((v) => ({ ...v, includeTranscript: on }))}
        />
      </div>
    </section>
  );
}
