'use client';

import Link from 'next/link';
import { Switch } from '@/components/ui/switch';
import { SettingsRow } from '@/components/Settings/SettingsRow';
import { SummaryLanguageSettings } from '@/components/SummaryLanguageSettings';
import { useConfig } from '@/contexts/ConfigContext';
import { settingsHref } from '@/lib/settings-panes';

export function SummariesPane() {
  const { isAutoSummary, toggleIsAutoSummary } = useConfig();
  return (
    <div>
      <SettingsRow label="Auto summary" htmlFor="auto-summary" helper="Generate the summary when a recording stops">
        <Switch id="auto-summary" aria-label="Auto summary" checked={isAutoSummary} onCheckedChange={toggleIsAutoSummary} />
      </SettingsRow>
      <SettingsRow label="Summary language" block>
        <SummaryLanguageSettings />
      </SettingsRow>
      <SettingsRow label="Summary model" helper="Set in Models & keys">
        <Link href={settingsHref('models')} className="text-sm underline">Models & keys</Link>
      </SettingsRow>
    </div>
  );
}
