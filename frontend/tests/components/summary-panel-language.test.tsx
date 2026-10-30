import { afterAll, beforeEach, describe, expect, mock, spyOn, test } from 'bun:test';
import { forwardRef, type ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingSummary, Transcript } from '../../src/types';
import { REGENERATE_DISCARD_PROMPT } from '../../src/lib/regenerate-guard';

const originals = {
  blockNote: { ...await import('../../src/components/AISummary/BlockNoteSummaryView') },
  empty: { ...await import('../../src/components/EmptyStateSummary') },
  generator: { ...await import('../../src/components/MeetingDetails/SummaryGeneratorButtonGroup') },
  updater: { ...await import('../../src/components/MeetingDetails/SummaryUpdaterButtonGroup') },
  popover: { ...await import('../../src/components/ui/popover') },
  picker: { ...await import('../../src/components/LanguagePickerPopover') },
  recents: { ...await import('../../src/hooks/useRecentLanguages') },
  preferences: { ...await import('../../src/lib/summary-language-preferences') },
  sonner: { ...await import('sonner') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('../../src/components/AISummary/BlockNoteSummaryView', () => originals.blockNote);
  mock.module('../../src/components/EmptyStateSummary', () => originals.empty);
  mock.module('../../src/components/MeetingDetails/SummaryGeneratorButtonGroup', () => originals.generator);
  mock.module('../../src/components/MeetingDetails/SummaryUpdaterButtonGroup', () => originals.updater);
  mock.module('../../src/components/ui/popover', () => originals.popover);
  mock.module('../../src/components/LanguagePickerPopover', () => originals.picker);
  mock.module('../../src/hooks/useRecentLanguages', () => originals.recents);
  mock.module('../../src/lib/summary-language-preferences', () => originals.preferences);
  mock.module('sonner', () => originals.sonner);
});

const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
const picker: { onChange?: (code: string | null) => void } = {};
const readLanguage = mock(async () => ({ language: 'en' as string | null, storage: 'metadata' as const }));
const saveLanguage = mock(async (_meetingId: string, language: string | null) => ({ language, storage: 'metadata' as const }));
const regenerate = mock(async () => {});

mock.module('../../src/components/AISummary/BlockNoteSummaryView', () => ({ BlockNoteSummaryView: forwardRef(() => null) }));
mock.module('../../src/components/EmptyStateSummary', () => ({ EmptyStateSummary: () => null }));
mock.module('../../src/components/MeetingDetails/SummaryGeneratorButtonGroup', () => ({
  SummaryGeneratorButtonGroup: (props: { languageSlot?: ReactNode }) => <>{props.languageSlot}</>,
}));
mock.module('../../src/components/MeetingDetails/SummaryUpdaterButtonGroup', () => ({ SummaryUpdaterButtonGroup: () => null }));
mock.module('../../src/components/ui/popover', () => ({ Popover: passThrough, PopoverTrigger: passThrough, PopoverContent: passThrough }));
mock.module('../../src/components/LanguagePickerPopover', () => ({
  LanguagePickerPopover: (props: { onChange: (code: string | null) => void }) => {
    picker.onChange = props.onChange;
    return null;
  },
}));
mock.module('../../src/hooks/useRecentLanguages', () => ({ useRecentLanguages: () => ({ addRecent: () => {} }) }));
mock.module('../../src/lib/summary-language-preferences', () => ({
  readMeetingSummaryLanguage: readLanguage,
  saveMeetingSummaryLanguage: saveLanguage,
}));
mock.module('sonner', () => ({ toast: { info: () => {}, error: () => {}, warning: () => {} } }));

const { SummaryPanel } = await import('../../src/components/MeetingDetails/SummaryPanel');

const summary = { markdown: 'Existing summary' } as MeetingSummary;
let renderer: ReactTestRenderer | null = null;

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

async function renderPanel(aiSummary: MeetingSummary | null, isSummaryDirty = false) {
  await act(async () => {
    renderer = create(
      <SummaryPanel
        meeting={{ id: 'meeting-a', title: 'Meeting A', created_at: '2026-10-14T00:00:00Z' }}
        meetingTitle="Meeting A"
        isSummaryDirty={isSummaryDirty}
        summaryRef={{ current: null }}
        isSaving={false}
        onSaveAll={async () => false}
        onCopySummary={async () => {}}
        aiSummary={aiSummary}
        summaryStatus="completed"
        transcripts={[{ text: 'Meeting transcript' } as Transcript]}
        modelConfig={{ provider: 'ollama', model: 'test', whisperModel: 'base' }}
        setModelConfig={() => {}}
        onSaveModelConfig={async () => {}}
        onGenerateSummary={async () => {}}
        onStopGeneration={() => {}}
        customPrompt=""
        onSaveSummary={async () => {}}
        onSummaryChange={() => {}}
        onDirtyChange={() => {}}
        summaryError={null}
        onRegenerateSummary={regenerate}
        getSummaryStatusMessage={() => ''}
        availableTemplates={[]}
        selectedTemplate={null}
        currentTemplate={null}
        languageFallback={false}
        onTemplateSelect={() => {}}
      />,
    );
  });
  await flush();
}

function languageButton(): ReactTestInstance {
  return renderer!.root.findAll((n) => n.type === 'button' && n.props['aria-label'] === 'Set summary language')[0];
}

async function chooseLanguage(code: string | null) {
  await act(async () => {
    picker.onChange?.(code);
  });
}

describe('summary panel language change', () => {
  beforeEach(() => {
    readLanguage.mockClear();
    saveLanguage.mockClear();
    regenerate.mockClear();
    saveLanguage.mockImplementation(async (_meetingId: string, language: string | null) => ({ language, storage: 'metadata' as const }));
    readLanguage.mockImplementation(async () => ({ language: 'en', storage: 'metadata' as const }));
    renderer?.unmount();
    renderer = null;
  });

  test('regenerates once when the language changes and a summary exists', async () => {
    await renderPanel(summary);
    await chooseLanguage('de');
    expect(saveLanguage).toHaveBeenCalledWith('meeting-a', 'de');
    expect(regenerate).toHaveBeenCalledTimes(1);
  });

  test('does not regenerate when there is no summary yet', async () => {
    await renderPanel(null);
    await chooseLanguage('de');
    expect(saveLanguage).toHaveBeenCalledWith('meeting-a', 'de');
    expect(regenerate).not.toHaveBeenCalled();
  });

  test('does not regenerate when the language is unchanged', async () => {
    await renderPanel(summary);
    await chooseLanguage('en');
    expect(saveLanguage).toHaveBeenCalledWith('meeting-a', 'en');
    expect(regenerate).not.toHaveBeenCalled();
  });

  test('does not regenerate when the save fails', async () => {
    const consoleError = spyOn(console, 'error').mockImplementation(() => {});
    saveLanguage.mockImplementation(async () => {
      throw new Error('disk full');
    });
    await renderPanel(summary);
    await chooseLanguage('de');
    consoleError.mockRestore();
    expect(regenerate).not.toHaveBeenCalled();
  });

  test('asks before regenerating over unsaved summary edits, and regenerates on Yes', async () => {
    const confirm = spyOn(window, 'confirm').mockImplementation(() => true);
    await renderPanel(summary, true);
    await chooseLanguage('de');
    expect(confirm).toHaveBeenCalledWith(REGENERATE_DISCARD_PROMPT);
    expect(regenerate).toHaveBeenCalledTimes(1);
    confirm.mockRestore();
  });

  test('keeps the new language saved but does not regenerate over unsaved edits on No', async () => {
    const confirm = spyOn(window, 'confirm').mockImplementation(() => false);
    await renderPanel(summary, true);
    await chooseLanguage('de');
    expect(saveLanguage).toHaveBeenCalledWith('meeting-a', 'de');
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(regenerate).not.toHaveBeenCalled();
    confirm.mockRestore();
  });

  test('the language picker is disabled until the stored language has loaded', async () => {
    let finishLoad: (stored: { language: string | null; storage: 'metadata' }) => void = () => {};
    readLanguage.mockImplementation(() => new Promise((resolve) => {
      finishLoad = resolve;
    }));
    await renderPanel(summary);
    expect(languageButton().props.disabled).toBe(true);
    await act(async () => {
      finishLoad({ language: 'en', storage: 'metadata' });
    });
    await flush();
    expect(languageButton().props.disabled).toBe(false);
  });

  test('the language picker is enabled once a failed language load has finished', async () => {
    const consoleError = spyOn(console, 'error').mockImplementation(() => {});
    readLanguage.mockImplementation(async () => {
      throw new Error('unreadable');
    });
    await renderPanel(summary);
    consoleError.mockRestore();
    expect(languageButton().props.disabled).toBe(false);
  });

  test('rapid switching regenerates once for the final choice', async () => {
    const pending: Array<{ language: string | null; resolve: () => void }> = [];
    saveLanguage.mockImplementation(async (_meetingId: string, language: string | null) => {
      await new Promise<void>((resolve) => pending.push({ language, resolve }));
      return { language, storage: 'metadata' as const };
    });
    await renderPanel(summary);
    await chooseLanguage('de');
    await chooseLanguage('fr');
    await act(async () => {
      pending.shift()?.resolve();
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(regenerate).not.toHaveBeenCalled();
    await act(async () => {
      pending.shift()?.resolve();
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(regenerate).toHaveBeenCalledTimes(1);
  });
});
