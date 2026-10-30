import { afterAll, afterEach, beforeEach, expect, mock, spyOn, test } from 'bun:test';
import { forwardRef, type ComponentProps, type ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  dialog: { ...await import('../../src/components/ui/dialog') },
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
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('../../src/components/ui/dialog', () => originals.dialog);
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
const readLanguage = mock(async () => ({ language: 'en' as string | null, storage: 'metadata' as const }));
const saveLanguage = mock(async (_meetingId: string, language: string | null) => ({ language, storage: 'metadata' as const }));
const regenerate = mock(async () => {});

mock.module('../../src/components/AISummary/BlockNoteSummaryView', () => ({ BlockNoteSummaryView: forwardRef(() => null) }));
mock.module('../../src/components/EmptyStateSummary', () => ({ EmptyStateSummary: () => null }));
mock.module('../../src/components/MeetingDetails/SummaryGeneratorButtonGroup', () => ({
  SummaryGeneratorButtonGroup: (props: { languageSlot?: ReactNode }) => <>{props.languageSlot}</>,
}));
mock.module('../../src/components/MeetingDetails/SummaryUpdaterButtonGroup', () => ({ SummaryUpdaterButtonGroup: ({ onSave }: { onSave: () => Promise<void> }) => <button onClick={onSave}>Save</button> }));
mock.module('../../src/components/ui/popover', () => ({ Popover: passThrough, PopoverTrigger: passThrough, PopoverContent: passThrough }));
mock.module('../../src/components/LanguagePickerPopover', () => ({ LanguagePickerPopover: () => null }));
mock.module('../../src/hooks/useRecentLanguages', () => ({ useRecentLanguages: () => ({ addRecent: () => {} }) }));
mock.module('../../src/lib/summary-language-preferences', () => ({
  readMeetingSummaryLanguage: readLanguage,
  saveMeetingSummaryLanguage: saveLanguage,
}));
const notifyError = mock(() => {});
mock.module('sonner', () => ({ toast: { info: () => {}, error: notifyError, warning: () => {} } }));

let settings = { provider: 'openai', model: 'test', includeTranscript: true, hideEditHint: false };
let failure: string | null = null;
const invoke = mock(async (command: string, _args?: unknown): Promise<unknown> => {
  if (command === failure) throw new Error('disk unavailable');
  switch (command) {
    case 'template_assist_get_settings': return settings;
    case 'template_assist_save_settings': return undefined;
    case 'api_template_meeting_info': return { templateId: 'one_on_one', hasEdits: true, transcriptTokens: 0 };
    case 'api_get_template': return { name: 'One on one', sections: [{ title: 'Notes', instruction: 'List notes' }] };
    case 'api_get_model_config': return undefined;
    case 'costs_get_prices': return [];
    case 'meeting_cost': case 'extraction_status': return null;
    default: throw new Error(`Unexpected command: ${command}`);
  }
});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('../../src/components/ui/dialog', () => ({
  Dialog: ({ open, children }: { open: boolean; children?: ReactNode }) => open ? <>{children}</> : null,
  DialogContent: passThrough, DialogHeader: passThrough, DialogTitle: passThrough,
  DialogDescription: passThrough, DialogFooter: passThrough,
}));
const { SummaryPanel } = await import('../../src/components/MeetingDetails/SummaryPanel');

const hint = 'You changed this summary. Suggest template changes?';
let renderer: ReactTestRenderer | undefined;
const text = (n: ReactTestInstance): string => n.children.map(c => typeof c === 'string' ? c : text(c)).join('');
const onSaveAll = mock(async () => true);
async function mount(available = true) {
  await act(async () => {
    renderer = create(<SummaryPanel
      meeting={{ id: 'meeting-a', title: 'Meeting A', created_at: '2026-10-14T00:00:00Z' }}
      meetingTitle="Meeting A" isSummaryDirty={true} summaryRef={{ current: null }} isSaving={false}
      onSaveAll={onSaveAll} onCopySummary={async () => {}} aiSummary={{ markdown: 'Existing summary' }}
      summaryStatus="completed" transcripts={[]}
      modelConfig={{ provider: 'ollama', model: 'test', whisperModel: 'base' }}
      setModelConfig={() => {}} onSaveModelConfig={async () => {}} onGenerateSummary={async () => {}}
      onStopGeneration={() => {}} customPrompt="" onSaveSummary={async () => {}}
      onSummaryChange={() => {}} onDirtyChange={() => {}} summaryError={null}
      onRegenerateSummary={regenerate} getSummaryStatusMessage={() => ''}
      availableTemplates={available ? [{ id: 'one_on_one', name: 'One on one' }] : []}
      selectedTemplate="one_on_one" currentTemplate={{ id: 'one_on_one', name: 'One on one', source: null }}
      languageFallback={false} onTemplateSelect={() => {}}
    />);
  });
}
async function click(label: string) {
  await act(async () => {
    await renderer!.root.findAll(n => n.type === 'button' && text(n) === label)[0].props.onClick();
  });
}
function hasHint() { return text(renderer!.root).includes(hint); }
beforeEach(() => {
  settings = { provider: 'openai', model: 'test', includeTranscript: true, hideEditHint: false };
  failure = null;
  invoke.mockClear(); notifyError.mockClear();
  onSaveAll.mockClear(); onSaveAll.mockImplementation(async () => true);
});
afterEach(() => {
  expect(invoke.mock.calls.filter(([command]) => command === 'api_template_revise')).toHaveLength(0);
  act(() => renderer?.unmount()); renderer = undefined;
});
test('shows the hint only after an edited summary is saved, loading settings once', async () => {
  await mount(); expect(hasHint()).toBe(false);
  await click('Save'); expect(hasHint()).toBe(true);
  expect(onSaveAll).toHaveBeenCalledTimes(1);
  expect(invoke.mock.calls.filter(([c]) => c === 'template_assist_get_settings')).toHaveLength(1);
});
test('does not carry a saved edit hint to another meeting in the same panel', async () => {
  await mount(); await click('Save'); expect(hasHint()).toBe(true);
  const props = renderer!.root.findByType(SummaryPanel).props as ComponentProps<typeof SummaryPanel>;
  await act(async () => {
    renderer!.update(<SummaryPanel {...props} meeting={{ ...props.meeting, id: 'meeting-b' }} />);
  });
  expect(hasHint()).toBe(false);
});
test('a regeneration clears the edit hint, so the completed summary does not bring it back', async () => {
  await mount(); await click('Save'); expect(hasHint()).toBe(true);
  const props = renderer!.root.findByType(SummaryPanel).props as ComponentProps<typeof SummaryPanel>;
  await act(async () => { renderer!.update(<SummaryPanel {...props} summaryStatus="regenerating" />); });
  await act(async () => { renderer!.update(<SummaryPanel {...props} summaryStatus="completed" />); });
  expect(hasHint()).toBe(false);
});
test('does not show the hint when save returns false', async () => {
  onSaveAll.mockImplementation(async () => false);
  await mount(); await click('Save'); expect(hasHint()).toBe(false);
});
test('does not show the hint when disabled in settings', async () => {
  settings.hideEditHint = true;
  await mount(); await click('Save'); expect(hasHint()).toBe(false);
});
test("Don't show again preserves settings, saves the preference, and hides the hint", async () => {
  await mount(); await click('Save'); await click("Don't show again");
  expect(invoke).toHaveBeenCalledWith('template_assist_save_settings', { settings: { ...settings, hideEditHint: true } });
  expect(hasHint()).toBe(false);
  await click('Save'); expect(hasHint()).toBe(false);
});
test('Dismiss hides the hint for this edit only', async () => {
  await mount(); await click('Save'); await click('Dismiss'); expect(hasHint()).toBe(false);
  await click('Save'); expect(hasHint()).toBe(true);
  expect(invoke.mock.calls.filter(([c]) => c === 'template_assist_save_settings')).toHaveLength(0);
});
test('Suggest opens the improve dialog and hides the hint without calling the LLM', async () => {
  await mount(); await click('Save'); await click('Suggest');
  expect(hasHint()).toBe(false);
  expect(text(renderer!.root)).toContain('What was wrong with this summary?');
});
test('does not show the hint where the improve action is unavailable', async () => {
  await mount(false); await click('Save'); expect(hasHint()).toBe(false);
});
test('a settings read failure is logged and suppresses the hint', async () => {
  failure = 'template_assist_get_settings';
  const error = spyOn(console, 'error').mockImplementation(() => {});
  try {
    await mount(); await click('Save'); expect(hasHint()).toBe(false);
    expect(error).toHaveBeenCalled();
  } finally { error.mockRestore(); }
});
test('a settings save failure keeps the hint and reports the error', async () => {
  await mount(); await click('Save'); failure = 'template_assist_save_settings';
  await click("Don't show again");
  expect(hasHint()).toBe(true); expect(notifyError).toHaveBeenCalled();
});
