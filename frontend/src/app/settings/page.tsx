'use client';

import React, { useEffect, Suspense } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { TranscriptSettings } from '@/components/TranscriptSettings';
import { RecordingSettings } from '@/components/RecordingSettings';
import { UpdatesSettings } from '@/components/UpdatesSettings';
import { PreferenceSettings } from '@/components/PreferenceSettings';
import { ModelsKeysPane } from '@/components/Settings/ModelsKeysPane';
import { SummariesPane } from '@/components/Settings/SummariesPane';
import { CopilotSettings } from '@/components/CopilotSettings';
import { KnowledgeSettings } from '@/components/KnowledgeSettings';
import { TemplateSettings } from '@/components/TemplateSettings';
import { TemplateAssistSettings } from '@/components/TemplateAssistSettings';
import { confirmLeave } from '@/lib/unsaved-guard';
import { SpeakerSettings } from '@/components/SpeakerSettings';
import { MODEL_DEFAULTS } from '@/constants/modelDefaults';
import { useConfig } from '@/contexts/ConfigContext';
import { PageFrame } from '@/components/PageFrame/PageFrame';
import { SettingsNav } from '@/components/Settings/SettingsNav';
import { paneForTab, paneLabel, settingId, settingsHref, type SettingsPane } from '@/lib/settings-panes';

function SettingsContent() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const { transcriptModelConfig, setTranscriptModelConfig } = useConfig();
  const pane = paneForTab(searchParams.get('tab'));

  const pick = async (p: SettingsPane, label?: string) => {
    if (!(await confirmLeave())) return;
    router.replace(settingsHref(p));
    if (label) requestAnimationFrame(() => document.getElementById(settingId(label))?.scrollIntoView());
  };

  // Load saved transcript configuration on mount
  useEffect(() => {
    const loadTranscriptConfig = async () => {
      try {
        const config = await invoke('api_get_transcript_config') as any;
        if (config) {
          console.log('Loaded saved transcript config:', { provider: config.provider, model: config.model });
          setTranscriptModelConfig({
            provider: config.provider || 'localWhisper',
            model: config.model || MODEL_DEFAULTS.localWhisper,
            apiKey: config.apiKey || null
          });
        }
      } catch (error) {
        console.error('Failed to load transcript config:', error);
      }
    };
    loadTranscriptConfig();
  }, [setTranscriptModelConfig]);

  let body: React.ReactNode;
  switch (pane) {
    case 'recording': body = <RecordingSettings />; break;
    case 'transcription': body = <TranscriptSettings transcriptModelConfig={transcriptModelConfig} setTranscriptModelConfig={setTranscriptModelConfig} />; break;
    case 'speakers': body = <SpeakerSettings />; break;
    case 'models': body = <ModelsKeysPane />; break;
    case 'summaries': body = <SummariesPane />; break;
    case 'copilot': body = <CopilotSettings />; break;
    case 'templates': body = <div className="space-y-6"><TemplateAssistSettings /><TemplateSettings /></div>; break;
    case 'names': body = <KnowledgeSettings />; break;
    case 'app': body = <div className="space-y-6"><PreferenceSettings /><UpdatesSettings /></div>; break;
  }

  return (
    <PageFrame variant={pane === 'templates' ? 'list' : 'form'} title={paneLabel(pane)} subnav={<SettingsNav pane={pane} onPick={pick} />}>
      {body}
    </PageFrame>
  );
}

export default function SettingsPage() {
  return (
    <Suspense fallback={null}>
      <SettingsContent />
    </Suspense>
  );
}
