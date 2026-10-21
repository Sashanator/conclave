import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { CLAUDE_FALLBACK_MODELS, GROQ_FALLBACK_MODELS, OPENAI_FALLBACK_MODELS } from '@/components/ModelSettingsModal';

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

/** A provider and model override. "Same as summaries" is null; the model saves on blur or Enter. */
export function LlmOverridePicker({
  idPrefix,
  providerLabel,
  modelLabel,
  provider,
  model,
  onChange,
}: {
  idPrefix: string;
  providerLabel: string;
  modelLabel: string;
  provider: string | null;
  model: string | null;
  onChange: (provider: string | null, model: string | null) => void;
}) {
  const [configured, setConfigured] = useState<Set<string>>(new Set());
  const [draft, setDraft] = useState(model ?? '');
  useEffect(() => {
    configuredProviders().then(setConfigured);
  }, []);
  useEffect(() => setDraft(model ?? ''), [model]);

  const providers = PROVIDERS.filter((p) => configured.has(p.id) || p.id === provider);
  const suggestions = provider ? MODEL_SUGGESTIONS[provider] ?? [] : [];

  const saveModel = () => {
    const next = draft.trim() || null;
    if (next !== (model ?? null)) onChange(provider, next);
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-4">
        <Label htmlFor={`${idPrefix}-provider`}>{providerLabel}</Label>
        <Select
          value={provider ?? SAME_AS_SUMMARIES}
          // The model belongs to the old provider, so it is cleared
          onValueChange={(next) => onChange(next === SAME_AS_SUMMARIES ? null : next, null)}
        >
          <SelectTrigger id={`${idPrefix}-provider`} className="w-56" aria-label={providerLabel}>
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
        <Label htmlFor={`${idPrefix}-model`}>{modelLabel}</Label>
        <Input
          id={`${idPrefix}-model`}
          className="w-56"
          list={`${idPrefix}-models`}
          disabled={!provider}
          placeholder={provider ? 'Model name' : 'Same as summaries'}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={saveModel}
          onKeyDown={(e) => {
            if (e.key === 'Enter') saveModel();
          }}
        />
        <datalist id={`${idPrefix}-models`}>
          {suggestions.map((m) => (
            <option key={m} value={m} />
          ))}
        </datalist>
      </div>
    </div>
  );
}
