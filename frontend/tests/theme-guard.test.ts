import { describe, expect, test } from 'bun:test';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const SRC = join(import.meta.dir, '..', 'src');

// Files not yet moved to theme tokens. Tasks 7-9 of the Phase 2 plan remove their files; Task 9 leaves this empty.
const PENDING = new Set<string>([
  'app/_components/SettingsModal.tsx',
  'app/_components/StatusOverlays.tsx',
  'app/_components/TranscriptPanel.tsx',
  'app/meeting-details/page-content.tsx',
  'app/notes/[id]/page.tsx',
  'app/page.tsx',
  'app/settings/page.tsx',
  'components/AISummary/Block.tsx',
  'components/AISummary/Section.tsx',
  'components/AISummary/index.tsx',
  'components/About.tsx',
  'components/AudioBackendSelector.tsx',
  'components/AudioLevelMeter.tsx',
  'components/BetaSettings.tsx',
  'components/BluetoothPlaybackWarning.tsx',
  'components/BuiltInModelManager.tsx',
  'components/ChunkProgressDisplay.tsx',
  'components/ComplianceNotification.tsx',
  'components/ConfirmationModel/confirmation-modal.tsx',
  'components/DatabaseImport/HomebrewDatabaseDetector.tsx',
  'components/DatabaseImport/LegacyDatabaseImport.tsx',
  'components/DeviceSelection.tsx',
  'components/EditableTitle.tsx',
  'components/EmptyStateSummary.tsx',
  'components/ImportAudio/ImportAudioDialog.tsx',
  'components/Info.tsx',
  'components/LanguagePickerPopover.tsx',
  'components/LanguageSelection.tsx',
  'components/Logo.tsx',
  'components/MeetingDetails/MeetingDetailsSplitView.tsx',
  'components/MeetingDetails/RetranscribeDialog.tsx',
  'components/MeetingDetails/SummaryGeneratorButtonGroup.tsx',
  'components/MeetingDetails/SummaryPanel.tsx',
  'components/MeetingDetails/SummaryUpdaterButtonGroup.tsx',
  'components/MeetingDetails/TranscriptButtonGroup.tsx',
  'components/MeetingDetails/TranscriptPanel.tsx',
  'components/ModelDownloadProgress.tsx',
  'components/ModelSettingsModal.tsx',
  'components/ParakeetModelManager.tsx',
  'components/PermissionWarning.tsx',
  'components/RecordingControls.tsx',
  'components/RecordingSettings.tsx',
  'components/RecordingStatusBar.tsx',
  'components/Sidebar/index.tsx',
  'components/SummaryLanguageSettings.tsx',
  'components/SummaryModelSettings.tsx',
  'components/TranscriptSettings.tsx',
  'components/TranscriptView.tsx',
  'components/VirtualizedTranscriptView.tsx',
  'components/WhisperModelManager.tsx',
  'components/molecules/form-components/form-input-item.tsx',
  'components/molecules/form-components/form-select-item.tsx',
  'components/onboarding/OnboardingContainer.tsx',
  'components/onboarding/shared/PermissionRow.tsx',
  'components/onboarding/shared/ProgressIndicator.tsx',
  'components/onboarding/shared/StatusIndicator.tsx',
  'components/onboarding/steps/DownloadProgressStep.tsx',
  'components/onboarding/steps/PermissionsStep.tsx',
  'components/onboarding/steps/SetupOverviewStep.tsx',
  'components/onboarding/steps/WelcomeStep.tsx',
  'components/shared/DownloadProgressToast.tsx',
  'components/ui/button.tsx',
  'lib/recordingNotification.tsx',
]);

// Neutral colours must be tokens (bg-background, text-muted-foreground, border-border…), never hard-coded.
const NEUTRAL =
  /(?<!dark:)\b(?:bg-white|(?:bg|text|border|divide|ring|ring-offset|placeholder|from|via|to|fill|stroke|outline)-(?:gray|slate|zinc|neutral|stone)-\d{2,3})\b/;
// Light tints of a hue need a dark: partner on the same line (e.g. bg-blue-50 dark:bg-blue-950/40).
const HUES = 'red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose';
const TINT = new RegExp(`(?<!dark:)\\b(?:bg-(?:${HUES})-(?:50|100|200)|text-(?:${HUES})-(?:700|800|900)|border-(?:${HUES})-(?:100|200|300))\\b`);

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) return sourceFiles(p);
    return /\.(tsx?|css)$/.test(name) ? [p] : [];
  });
}

function offenders(file: string): string[] {
  return readFileSync(file, 'utf8')
    .split('\n')
    .flatMap((line, i) => (NEUTRAL.test(line) || (TINT.test(line) && !line.includes('dark:')) ? [`${i + 1}: ${line.trim()}`] : []));
}

describe('theme guard', () => {
  const files = sourceFiles(SRC).map((f) => [relative(SRC, f).split('\\').join('/'), f] as const);

  test('converted files use theme tokens', () => {
    const bad = files.filter(([rel]) => !PENDING.has(rel)).flatMap(([rel, f]) => offenders(f).map((o) => `${rel}:${o}`));
    expect(bad).toEqual([]);
  });

  test('every pending file still needs converting (keep the list honest)', () => {
    const stale = [...PENDING].filter((rel) => {
      const hit = files.find(([r]) => r === rel);
      return !hit || offenders(hit[1]).length === 0;
    });
    expect(stale).toEqual([]);
  });
});
