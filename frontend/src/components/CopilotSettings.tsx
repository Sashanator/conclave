'use client';

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { X } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { ModelLinkRow, SettingsRow, useSavedTick } from '@/components/Settings/SettingsRow';
import { addAlertWord, resolveAnswerLanguage, type CopilotSettings } from '@/lib/copilot';
import { readPinnedSummaryLanguageDefault } from '@/lib/summary-language-preferences';
import { LANGUAGE_OPTIONS } from '@/lib/summary-languages';

export function CopilotSettings({ part }: { part: 'answer' | 'alert-words' }) {
  const [settings, setSettings] = useState<CopilotSettings | null>(null);
  const [word, setWord] = useState('');
  const [saved, markSaved] = useSavedTick();
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
        const result = await invoke<CopilotSettings>('copilot_save_settings', { settings: next });
        // A newer edit has already replaced this one, so only the newest result is shown
        if (latest.current === next) show(result);
        markSaved();
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

  if (part === 'answer') {
    return (
      <div>
        <SettingsRow label="Answer language" htmlFor="copilot-answer-language" saved={saved}>
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
        </SettingsRow>
        <ModelLinkRow />
      </div>
    );
  }

  return (
    <SettingsRow
      label="Alert me when someone says"
      htmlFor="copilot-alert-word"
      block
      saved={saved}
      helper="Add the forms people use, e.g. Sasha, Саша, Alexander, Александр. A word also matches longer words that start with it."
    >
      <div className="space-y-2">
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
      </div>
    </SettingsRow>
  );
}
