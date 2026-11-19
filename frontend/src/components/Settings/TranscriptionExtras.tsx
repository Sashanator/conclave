'use client';

import Link from 'next/link';
import { Switch } from '@/components/ui/switch';
import { SettingsRow } from '@/components/Settings/SettingsRow';
import { useConfig } from '@/contexts/ConfigContext';

export function TranscriptionExtras() {
  const { betaFeatures, toggleBetaFeature } = useConfig();
  return (
    <>
      <SettingsRow label="Import audio & retranscribe" htmlFor="import-retranscribe" helper="Shows Import audio in the nav and Retranscribe on meetings">
        <Switch
          id="import-retranscribe"
          aria-label="Import audio & retranscribe"
          checked={betaFeatures.importAndRetranscribe}
          onCheckedChange={(on) => toggleBetaFeature('importAndRetranscribe', on)}
        />
      </SettingsRow>
      <SettingsRow label="Vocabulary" helper="Words Conclave should spell your way, in dictation and in meetings">
        <Link href="/dictate" className="text-sm underline">Edit on Dictate</Link>
      </SettingsRow>
    </>
  );
}
