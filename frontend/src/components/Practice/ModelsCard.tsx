import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { estimateCost, type ChatModel, type PlanEffort, type PlanModel, type PracticeSettings } from '@/lib/practice';

const CHAT_MODELS: { id: ChatModel; label: string }[] = [
  { id: 'claude-haiku-5-5', label: 'Claude Haiku 5.5' },
  { id: 'claude-sonnet-5-5', label: 'Claude Sonnet 5.5' },
  { id: 'claude-opus-5-5', label: 'Claude Opus 5.5' },
];
const PLAN_MODELS: { id: PlanModel; label: string }[] = [
  { id: 'claude-opus-5-5', label: 'Claude Opus 5.5' },
  { id: 'claude-sonnet-5-5', label: 'Claude Sonnet 5.5' },
];
const EFFORTS: { id: PlanEffort; label: string }[] = [
  { id: 'medium', label: 'Medium' },
  { id: 'high', label: 'High' },
  { id: 'xhigh', label: 'Extra high' },
];

// The Models card on the Practice page. Saves follow the Dictate page: edits show at once, saves run in order.
export function ModelsCard() {
  const [settings, setSettings] = useState<PracticeSettings | null>(null);
  // Edits show at once. Saves run one after another, each from the latest edit.
  const latest = useRef<PracticeSettings | null>(null);
  // The last settings the backend confirmed (loaded or saved). A failed reload falls back to them.
  const confirmed = useRef<PracticeSettings | null>(null);
  const queue = useRef<Promise<void>>(Promise.resolve());
  // Bumped when a save fails: saves queued before the reload are dropped, the reload wins
  const generation = useRef(0);

  const show = (s: PracticeSettings) => {
    latest.current = s;
    setSettings(s);
  };

  const load = async () => {
    try {
      const loaded = await invoke<PracticeSettings>('practice_get_settings');
      confirmed.current = loaded;
      show(loaded);
    } catch (err) {
      toast.error('Could not load Practice models', { description: String(err) });
      // The shown copy may hold a rejected edit: go back to what the backend last confirmed
      if (confirmed.current) show(confirmed.current);
    }
  };

  useEffect(() => {
    load();
  }, []);

  const commit = (change: (s: PracticeSettings) => PracticeSettings) => {
    if (!latest.current) return;
    const next = change(latest.current);
    const gen = generation.current;
    show(next);
    queue.current = queue.current.then(async () => {
      try {
        if (gen !== generation.current) return;
        const saved = await invoke<PracticeSettings>('practice_save_settings', { settings: next });
        confirmed.current = saved;
        // A newer edit has already replaced this one, so only the newest result is shown
        if (latest.current === next) show(saved);
      } catch (err) {
        toast.error('Practice models were not saved', { description: String(err) });
        await load();
        // Bumped after the reload: edits made during it are queued under the old generation and dropped too
        generation.current++;
      }
    });
  };

  if (!settings) return null;

  const chatCost = estimateCost(settings.chat_model, 'chat');
  const planCost = estimateCost(settings.plan_model, 'plan_report');

  return (
    <section className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-5">
      <h3 className="text-lg font-semibold text-foreground">Models</h3>

      <div className="flex items-center justify-between gap-4">
        <div>
          <Label htmlFor="practice-chat-model">Conversation</Label>
          <p className="text-sm text-muted-foreground">{`≈ $${chatCost.toFixed(2)} per 30-min session`}</p>
        </div>
        <Select value={settings.chat_model} onValueChange={(id) => commit((s) => ({ ...s, chat_model: id as ChatModel }))}>
          <SelectTrigger id="practice-chat-model" aria-label="Conversation" className="w-56">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {CHAT_MODELS.map((m) => (
              <SelectItem key={m.id} value={m.id}>{m.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      {settings.chat_model === 'claude-opus-5-5' && (
        <p className="text-sm text-amber-700 dark:text-amber-400">Slower replies</p>
      )}

      <div className="flex items-center justify-between gap-4">
        <div>
          <Label htmlFor="practice-plan-model">Plan and report</Label>
          <p className="text-sm text-muted-foreground">{`≈ $${planCost.toFixed(2)} per 30-min session`}</p>
        </div>
        <Select value={settings.plan_model} onValueChange={(id) => commit((s) => ({ ...s, plan_model: id as PlanModel }))}>
          <SelectTrigger id="practice-plan-model" aria-label="Plan and report" className="w-56">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PLAN_MODELS.map((m) => (
              <SelectItem key={m.id} value={m.id}>{m.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="flex items-center justify-between gap-4">
        <div>
          <Label htmlFor="practice-plan-effort">Effort</Label>
          <p className="text-sm text-muted-foreground">{`≈ $${planCost.toFixed(2)} per 30-min session`}</p>
        </div>
        <Select value={settings.plan_effort} onValueChange={(id) => commit((s) => ({ ...s, plan_effort: id as PlanEffort }))}>
          <SelectTrigger id="practice-plan-effort" aria-label="Effort" className="w-56">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {EFFORTS.map((m) => (
              <SelectItem key={m.id} value={m.id}>{m.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
    </section>
  );
}
