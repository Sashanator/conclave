import { invoke } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { getAssistSettings, saveAssistSettings } from '@/lib/templateAssist';

// Saves one provider's key only. Re-announces the summary config so ConfigContext and the Summary editor drop their stale key copies.
export async function saveProviderKey(provider: string, apiKey: string): Promise<void> {
  await invoke('api_save_provider_key', { provider, apiKey });
  const config = await invoke('api_get_model_config');
  if (config) await emit('model-config-updated', config);
}

export type ModelFeature = 'copilot' | 'knowledge' | 'templateAssist' | 'dictateCleanup';
// null = "Same as Summary"
export type FeatureChoice = { provider: string; model: string } | null;

export function featureChoice(provider?: string | null, model?: string | null): FeatureChoice {
  const p = provider?.trim();
  const m = model?.trim();
  return p && m ? { provider: p, model: m } : null;
}

type Pair = { provider?: string | null; model?: string | null };
type Obj = Record<string, unknown>;

export async function loadFeatureModels(): Promise<Record<ModelFeature, FeatureChoice>> {
  const [c, k, t, d] = await Promise.all([
    invoke<Pair>('copilot_get_settings'),
    invoke<Pair>('knowledge_get_settings'),
    getAssistSettings(),
    invoke<{ cleanup_provider?: string | null; cleanup_model?: string | null }>('dictate_get_settings'),
  ]);
  return {
    copilot: featureChoice(c.provider, c.model),
    knowledge: featureChoice(k.provider, k.model),
    templateAssist: featureChoice(t.provider, t.model),
    dictateCleanup: featureChoice(d.cleanup_provider, d.cleanup_model),
  };
}

// Read-modify-write of the feature's own settings, so its other fields are never written from a stale copy.
export async function saveFeatureModel(feature: ModelFeature, choice: FeatureChoice): Promise<void> {
  const provider = choice?.provider ?? null;
  const model = choice?.model ?? null;
  switch (feature) {
    case 'copilot': {
      const s = await invoke<Obj>('copilot_get_settings');
      return void (await invoke('copilot_save_settings', { settings: { ...s, provider, model } }));
    }
    case 'knowledge': {
      // calendar_host is view-only; the save input does not take it.
      const { calendar_host: _host, ...s } = await invoke<Obj>('knowledge_get_settings');
      return void (await invoke('knowledge_save_settings', { input: { ...s, provider, model } }));
    }
    case 'templateAssist': {
      const s = await getAssistSettings();
      return saveAssistSettings({ ...s, provider, model });
    }
    case 'dictateCleanup': {
      const s = await invoke<Obj>('dictate_get_settings');
      return void (await invoke('dictate_save_settings', { settings: { ...s, cleanup_provider: provider, cleanup_model: model } }));
    }
  }
}
