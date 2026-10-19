'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { DictateGeneral } from '@/components/Dictate/DictateGeneral';
import { ModeList } from '@/components/Dictate/ModeList';
import { DictionaryEditor } from '@/components/Dictate/DictionaryEditor';
import type { DictateSettings, DictateStatus } from '@/lib/dictate';

export default function DictatePage() {
  const [settings, setSettings] = useState<DictateSettings | null>(null);
  const [status, setStatus] = useState<DictateStatus | null>(null);
  // Edits show at once. Saves run one after another, each from the latest edit.
  const latest = useRef<DictateSettings | null>(null);
  const queue = useRef<Promise<void>>(Promise.resolve());
  // Bumped when a save fails: saves queued before the reload are dropped, the reload wins
  const generation = useRef(0);
  // Saves queued or running. The tray can change settings meanwhile; then the page reloads after them.
  const pending = useRef(0);

  const show = (s: DictateSettings) => {
    latest.current = s;
    setSettings(s);
  };

  const load = useCallback(async () => {
    try {
      show(await invoke<DictateSettings>('dictate_get_settings'));
    } catch (err) {
      toast.error('Could not load Dictate settings', { description: String(err) });
    }
  }, []);

  useEffect(() => {
    load();
    invoke<DictateStatus>('dictate_status')
      .then(setStatus)
      .catch((err) => toast.error('Could not read Dictate status', { description: String(err) }));
    const unlisten = listen<DictateSettings>('dictate-settings-changed', (e) => {
      if (pending.current === 0) show(e.payload);
      else queue.current = queue.current.then(load);
    });
    return () => {
      unlisten.then((stop) => stop());
    };
  }, [load]);

  const commit = (change: (s: DictateSettings) => DictateSettings) => {
    if (!latest.current) return;
    const next = change(latest.current);
    const gen = generation.current;
    show(next);
    pending.current++;
    queue.current = queue.current.then(async () => {
      try {
        if (gen !== generation.current) return;
        const saved = await invoke<DictateSettings>('dictate_save_settings', { settings: next });
        // A newer edit has already replaced this one, so only the newest result is shown
        if (latest.current === next) show(saved);
      } catch (err) {
        toast.error('Dictate settings were not saved', { description: String(err) });
        await load();
        // Bumped after the reload: edits made during it are queued under the old generation and dropped too
        generation.current++;
      } finally {
        pending.current--;
      }
    });
  };

  if (!settings) {
    return <div className="p-8 text-muted-foreground">Loading…</div>;
  }

  return (
    <div className="h-full overflow-y-auto">
      <div className="max-w-3xl mx-auto p-8 space-y-6">
        <h1 className="text-3xl font-bold">Dictate</h1>
        <DictateGeneral settings={settings} status={status} commit={commit} />
        <ModeList settings={settings} commit={commit} />
        <DictionaryEditor settings={settings} commit={commit} />
      </div>
    </div>
  );
}
