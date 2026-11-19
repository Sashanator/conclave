'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { SettingsRow, useSavedTick } from '@/components/Settings/SettingsRow';
import { SummaryModelSettings } from '@/components/SummaryModelSettings';
import { CostsSettings } from '@/components/CostsSettings';
import { ModelsCard } from '@/components/Practice/ModelsCard';
import { LlmOverridePicker, PROVIDERS, configuredProviders } from '@/components/shared/LlmOverridePicker';
import { useConfig } from '@/contexts/ConfigContext';
import { saveProviderKey, loadFeatureModels, saveFeatureModel, featureChoice, type FeatureChoice, type ModelFeature } from '@/lib/models-keys';

const KEY_PROVIDERS = ['claude', 'openai', 'groq', 'openrouter'];
const FEATURES: { feature: ModelFeature; label: string; id: string }[] = [
  { feature: 'copilot', label: 'Copilot', id: 'mk-copilot' },
  { feature: 'knowledge', label: 'Tasks & digests', id: 'mk-knowledge' },
  { feature: 'templateAssist', label: 'Template assistant', id: 'mk-template' },
  { feature: 'dictateCleanup', label: 'Dictate cleanup', id: 'mk-dictate' },
];

function KeyRow({ label, name, isSet, helper, canSet, onSave }: {
  label: string; name: string; isSet: boolean; helper?: string; canSet: boolean; onSave: (key: string) => Promise<boolean>;
}) {
  const [saved, markSaved] = useSavedTick();
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState('');
  const save = async () => {
    if (!value.trim()) return;
    if (await onSave(value.trim())) {
      setValue('');
      setOpen(false);
      markSaved();
    }
  };
  return (
    <SettingsRow label={label} saved={saved} helper={helper ?? (isSet ? 'Key set' : 'Not set')}>
      {canSet && (open ? (
        <div className="flex items-center gap-2">
          <Input type="password" aria-label={`${name} API key`} className="w-56" autoComplete="off" value={value} onChange={(e) => setValue(e.target.value)} />
          <Button size="sm" onClick={save}>Save</Button>
          <Button size="sm" variant="outline" onClick={() => { setOpen(false); setValue(''); }}>Cancel</Button>
        </div>
      ) : (
        <Button size="sm" variant="outline" onClick={() => setOpen(true)}>{isSet ? 'Replace' : 'Add key'}</Button>
      ))}
    </SettingsRow>
  );
}

function FeatureRow({ feature, label, id, choice, onSaved }: {
  feature: ModelFeature; label: string; id: string; choice: FeatureChoice; onSaved: (c: FeatureChoice) => void;
}) {
  const [saved, markSaved] = useSavedTick();
  // A provider picked without a model is shown but not saved: the backend takes both or neither.
  const [pending, setPending] = useState<string | null>(null);
  const queue = useRef<Promise<void>>(Promise.resolve());
  const change = (provider: string | null, model: string | null) => {
    if (provider && !model) return setPending(provider);
    setPending(null);
    const next = featureChoice(provider, model);
    queue.current = queue.current.then(async () => {
      try {
        await saveFeatureModel(feature, next);
        onSaved(next);
        markSaved();
      } catch (err) {
        toast.error(`${label} model was not saved`, { description: String(err) });
      }
    });
  };
  return (
    <SettingsRow label={label} block saved={saved}>
      <LlmOverridePicker idPrefix={id} providerLabel={`${label} provider`} modelLabel={`${label} model`}
        provider={pending ?? choice?.provider ?? null} model={pending ? null : choice?.model ?? null} onChange={change} />
    </SettingsRow>
  );
}

export function ModelsKeysPane() {
  const { updateProviderApiKey } = useConfig();
  const [keysVersion, setKeysVersion] = useState(0);
  const [configured, setConfigured] = useState<Set<string>>(new Set());
  const [customHost, setCustomHost] = useState<string | null>(null);
  const [choices, setChoices] = useState<Record<ModelFeature, FeatureChoice> | null>(null);
  const [summarySaved, markSummarySaved] = useSavedTick();

  useEffect(() => {
    configuredProviders().then(setConfigured);
    invoke<{ endpoint?: string } | null>('api_get_custom_openai_config')
      .then((c) => {
        if (!c) return setCustomHost(null);
        try { setCustomHost(new URL(c.endpoint ?? '').host || 'custom server'); } catch { setCustomHost('custom server'); }
      })
      .catch(() => setCustomHost(null));
  }, [keysVersion]);

  useEffect(() => {
    loadFeatureModels().then(setChoices).catch((err) => toast.error('Could not load model choices', { description: String(err) }));
  }, []);

  const saveKey = useCallback(async (provider: string, key: string) => {
    try {
      await saveProviderKey(provider, key);
      updateProviderApiKey(provider, key);
      setKeysVersion((v) => v + 1);
      return true;
    } catch (err) {
      toast.error('Key was not saved', { description: String(err) });
      return false;
    }
  }, [updateProviderApiKey]);

  const bump = () => setKeysVersion((v) => v + 1);

  return (
    <div className="space-y-8">
      <section>
        <h2 className="text-lg font-semibold">Providers</h2>
        {KEY_PROVIDERS.map((id) => {
          const label = PROVIDERS.find((p) => p.id === id)!.label;
          return <KeyRow key={id} label={label} name={label} isSet={configured.has(id)} canSet onSave={(k) => saveKey(id, k)} />;
        })}
        <KeyRow label="Custom server" name="Custom server" isSet={!!customHost} canSet={!!customHost}
          helper={customHost ?? 'Set it up in the Summary row: choose Custom Server (OpenAI)'} onSave={(k) => saveKey('custom-openai', k)} />
      </section>

      <section>
        <h2 className="text-lg font-semibold">Which model each feature uses</h2>
        <SettingsRow label="Summary" block saved={summarySaved}>
          <SummaryModelSettings key={keysVersion} onSaved={() => { markSummarySaved(); bump(); }} />
        </SettingsRow>
        {choices && FEATURES.map((f) => (
          <FeatureRow key={f.feature} {...f} choice={choices[f.feature]} onSaved={(c) => setChoices((s) => (s ? { ...s, [f.feature]: c } : s))} />
        ))}
        <SettingsRow label="Practice" helper="Claude only: role-play partner, plan and report" block>
          <ModelsCard />
        </SettingsRow>
      </section>

      <section>
        <h2 className="text-lg font-semibold">Prices (per 1M tokens)</h2>
        <CostsSettings key={keysVersion} />
      </section>
    </div>
  );
}
