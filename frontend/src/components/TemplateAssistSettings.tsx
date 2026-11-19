'use client';

import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { Switch } from '@/components/ui/switch';
import { ModelLinkRow, SettingsRow, useSavedTick } from '@/components/Settings/SettingsRow';
import { getAssistSettings, saveAssistSettings, type TemplateAssistSettings as AssistSettings } from '@/lib/templateAssist';

export function TemplateAssistSettings() {
  const [view, setView] = useState<AssistSettings | null>(null);
  const [saved, markSaved] = useSavedTick();
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
        // Only includeTranscript is this pane's; the model and the hidden hint flag come from the current settings.
        await saveAssistSettings({ ...(await getAssistSettings()), includeTranscript: next.includeTranscript });
        markSaved();
      } catch (err) {
        toast.error('Template assistant settings were not saved', { description: String(err) });
        await load();
        generation.current++;
      }
    });
  };

  if (!view) return null;

  return (
    <div>
      <SettingsRow label="Include the transcript by default" htmlFor="template-assist-transcript" saved={saved}>
        <Switch
          id="template-assist-transcript"
          aria-label="Include the transcript by default"
          checked={view.includeTranscript}
          onCheckedChange={(on) => commit((v) => ({ ...v, includeTranscript: on }))}
        />
      </SettingsRow>
      <ModelLinkRow />
    </div>
  );
}
