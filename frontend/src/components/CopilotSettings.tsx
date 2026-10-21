'use client';

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { X } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { LlmOverridePicker } from '@/components/shared/LlmOverridePicker';
import { addAlertWord, resolveAnswerLanguage, type CopilotSettings } from '@/lib/copilot';
import { readPinnedSummaryLanguageDefault } from '@/lib/summary-language-preferences';
import { LANGUAGE_OPTIONS } from '@/lib/summary-languages';

export function CopilotSettings() {
  const [settings, setSettings] = useState<CopilotSettings | null>(null);
  const [word, setWord] = useState('');
  // A provider picked without a model. Shown but not saved: the backend takes a provider and model together or neither.
  const [pendingProvider, setPendingProvider] = useState<string | null>(null);
  // Edits show at once. Saves run one after another, each from the latest edit.
  const latest = useRef<CopilotSettings | null>(null);
  const queue = useRef<Promise<void>>(Promise.resolve());
  // Bumped after a failed save and its reload: saves queued before that are dropped
  const generation = useRef(0);

  const show = (s: CopilotSettings) => {
    latest.current = s;
    setSettings(s);
  };

  const load = async () => {
    try {
      show(await invoke<CopilotSettings>('copilot_get_settings'));
    } catch (err) {
      toast.error('Could not load Copilot settings', { description: String(err) });
    }
  };

  useEffect(() => {
    load();
  }, []);

  const commit = (change: (s: CopilotSettings) => CopilotSettings) => {
    if (!latest.current) return;
    const next = change(latest.current);
    const gen = generation.current;
    show(next);
    queue.current = queue.current.then(async () => {
      if (gen !== generation.current) return;
      try {
        const saved = await invoke<CopilotSettings>('copilot_save_settings', { settings: next });
        // A newer edit has already replaced this one, so only the newest result is shown
        if (latest.current === next) show(saved);
      } catch (err) {
        toast.error('Copilot settings were not saved', { description: String(err) });
        await load();
        generation.current++;
      }
    });
  };

  if (!settings) return null;

  const addWord = () => {
    if (addAlertWord(settings.alert_words, word) === settings.alert_words) return; // rejected: keep the text so it can be fixed
    commit((s) => ({ ...s, alert_words: addAlertWord(s.alert_words, word) }));
    setWord('');
  };

  return (
    <section className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-5">
      <h3 className="text-lg font-semibold text-foreground">Live copilot</h3>

      <LlmOverridePicker
        idPrefix="copilot"
        providerLabel="Copilot provider"
        modelLabel="Copilot model"
        provider={pendingProvider ?? settings.provider}
        model={pendingProvider ? null : settings.model}
        onChange={(provider, model) => {
          if (provider && !model) {
            setPendingProvider(provider);
            return;
          }
          setPendingProvider(null);
          commit((s) => ({ ...s, provider, model }));
        }}
      />

      <div className="flex items-center justify-between gap-4">
        <Label htmlFor="copilot-answer-language">Answer language</Label>
        <Select
          value={resolveAnswerLanguage(settings.answer_language, readPinnedSummaryLanguageDefault())}
          onValueChange={(code) => commit((s) => ({ ...s, answer_language: code }))}
        >
          <SelectTrigger id="copilot-answer-language" className="w-56" aria-label="Answer language">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {LANGUAGE_OPTIONS.map((o) => (
              <SelectItem key={o.code} value={o.code}>{o.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="space-y-2">
        <Label htmlFor="copilot-alert-word">Alert me when someone says</Label>
        <Input
          id="copilot-alert-word"
          aria-label="Add an alert word"
          placeholder="Add a word and press Enter"
          value={word}
          onChange={(e) => setWord(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              addWord();
            }
          }}
        />

        <ul className="flex flex-wrap gap-2">
          {settings.alert_words.map((w) => (
            <li key={w} className="flex items-center gap-1.5 rounded-full border border-border px-3 py-1 text-sm">
              {w}
              <button
                type="button"
                aria-label={`Remove ${w}`}
                className="text-muted-foreground hover:text-foreground"
                onClick={() => commit((s) => ({ ...s, alert_words: s.alert_words.filter((x) => x !== w) }))}
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </li>
          ))}
        </ul>

        <p className="text-xs text-muted-foreground">
          Add the forms people use, e.g. Sasha, Саша, Alexander, Александр. A word also matches longer words that start with it.
        </p>
      </div>
    </section>
  );
}
