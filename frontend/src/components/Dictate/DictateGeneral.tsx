import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { CLAUDE_FALLBACK_MODELS, GROQ_FALLBACK_MODELS, OPENAI_FALLBACK_MODELS } from '@/components/ModelSettingsModal';
import { BUILT_IN_MODES, hotkeyProblem, withHotkey, type Commit, type DictateSettings, type DictateStatus } from '@/lib/dictate';
import { HotkeyPicker } from './HotkeyPicker';

// Built-in "builtin-ai" is left out: the backend rejects it as a cleanup provider
const PROVIDERS = [
  { id: 'claude', label: 'Claude' },
  { id: 'openai', label: 'OpenAI' },
  { id: 'groq', label: 'Groq' },
  { id: 'openrouter', label: 'OpenRouter' },
  { id: 'ollama', label: 'Ollama' },
  { id: 'custom-openai', label: 'Custom Server (OpenAI)' },
];
const MODEL_SUGGESTIONS: Record<string, string[]> = {
  claude: CLAUDE_FALLBACK_MODELS,
  openai: OPENAI_FALLBACK_MODELS,
  groq: GROQ_FALLBACK_MODELS,
};
const SAME_AS_SUMMARIES = 'same';

// Providers with a key, plus ollama or a custom server when configured. A provider already chosen always stays listed.
async function configuredProviders(): Promise<Set<string>> {
  const found = new Set<string>();
  for (const provider of ['claude', 'openai', 'groq', 'openrouter']) {
    try {
      if (await invoke<string>('api_get_api_key', { provider })) found.add(provider);
    } catch {
      // No key stored for this provider
    }
  }
  try {
    const config = await invoke<{ provider?: string; ollamaEndpoint?: string | null } | null>('api_get_model_config');
    if (config?.provider === 'ollama' || config?.ollamaEndpoint) found.add('ollama');
  } catch {
    // No model config saved yet
  }
  try {
    if (await invoke('api_get_custom_openai_config')) found.add('custom-openai');
  } catch {
    // No custom server configured
  }
  return found;
}

export function DictateGeneral({ settings, status, commit }: { settings: DictateSettings; status: DictateStatus | null; commit: Commit }) {
  const [configured, setConfigured] = useState<Set<string>>(new Set());
  const [model, setModel] = useState(settings.cleanup_model ?? '');
  useEffect(() => {
    configuredProviders().then(setConfigured);
  }, []);
  useEffect(() => setModel(settings.cleanup_model ?? ''), [settings.cleanup_model]);

  const blocked = !status || !status.engine_is_whisper || !status.model_downloaded;
  const providers = PROVIDERS.filter((p) => configured.has(p.id) || p.id === settings.cleanup_provider);
  const suggestions = settings.cleanup_provider ? MODEL_SUGGESTIONS[settings.cleanup_provider] ?? [] : [];

  const saveModel = () => {
    const next = model.trim() || null;
    if (next !== (settings.cleanup_model ?? null)) commit((s) => ({ ...s, cleanup_model: next }));
  };

  return (
    <section className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-5">
      <h3 className="text-lg font-semibold text-foreground">General</h3>

      <div className="flex items-center justify-between gap-4">
        <div>
          <Label htmlFor="dictate-enabled">Dictate</Label>
          <p className="text-sm text-muted-foreground">Dictate hotkeys work in any app while Conclave is running</p>
        </div>
        <Switch
          id="dictate-enabled"
          aria-label="Dictate"
          checked={settings.enabled}
          // Turning it off stays possible when the engine or model is missing
          disabled={!status || (blocked && !settings.enabled)}
          onCheckedChange={(enabled) => commit((s) => ({ ...s, enabled }))}
        />
      </div>
      {status?.reason && <p className="text-sm text-muted-foreground">{status.reason}</p>}
      <p className="text-sm text-muted-foreground">
        Hold the hotkey to talk, or tap it to start and tap again to stop. Esc cancels. Doesn&apos;t work while an app running as administrator, or a VMware virtual machine that has captured the keyboard, is in front. The microphone stays on while Dictate is on.
      </p>

      <div className="space-y-1.5">
        <Label>Main hotkey</Label>
        <HotkeyPicker
          label="Main hotkey"
          value={settings.main_hotkey}
          check={(h) => hotkeyProblem(withHotkey(settings, 'main', h))}
          onChange={(h) => commit((s) => withHotkey(s, 'main', h))}
        />
      </div>

      <div className="flex items-center justify-between gap-4">
        <Label>Default mode</Label>
        <Select value={settings.default_mode} onValueChange={(mode) => commit((s) => ({ ...s, default_mode: mode }))}>
          <SelectTrigger className="w-56" aria-label="Default mode">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {BUILT_IN_MODES.map((m) => (
              <SelectItem key={m.id} value={m.id}>{m.name}</SelectItem>
            ))}
            {settings.modes.map((m) => (
              <SelectItem key={m.id} value={m.id}>{m.name}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="space-y-2">
        <div className="flex items-center justify-between gap-4">
          <div>
            <Label>Cleanup provider</Label>
            <p className="text-sm text-muted-foreground">Used by every mode except &quot;As spoken&quot;</p>
          </div>
          <Select
            value={settings.cleanup_provider ?? SAME_AS_SUMMARIES}
            onValueChange={(provider) =>
              // The model belongs to the old provider, so it is cleared
              commit((s) => ({ ...s, cleanup_provider: provider === SAME_AS_SUMMARIES ? null : provider, cleanup_model: null }))
            }
          >
            <SelectTrigger className="w-56" aria-label="Cleanup provider">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={SAME_AS_SUMMARIES}>Same as summaries</SelectItem>
              {providers.map((p) => (
                <SelectItem key={p.id} value={p.id}>{p.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex items-center justify-between gap-4">
          <Label htmlFor="dictate-cleanup-model">Cleanup model</Label>
          <Input
            id="dictate-cleanup-model"
            className="w-56"
            list="dictate-cleanup-models"
            disabled={!settings.cleanup_provider}
            placeholder={settings.cleanup_provider ? 'Model name' : 'Same as summaries'}
            value={model}
            onChange={(e) => setModel(e.target.value)}
            onBlur={saveModel}
            onKeyDown={(e) => {
              if (e.key === 'Enter') saveModel();
            }}
          />
          <datalist id="dictate-cleanup-models">
            {suggestions.map((m) => (
              <option key={m} value={m} />
            ))}
          </datalist>
        </div>
      </div>
    </section>
  );
}
