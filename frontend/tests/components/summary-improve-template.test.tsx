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
const regenerate = mock(async (_templateId?: string) => {});

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

const base = { name: 'One on one', sections: [{ title: 'Notes', instruction: 'List notes' }] };
let settings = { provider: 'openai', model: 'test', includeTranscript: true, hideEditHint: false };
let failure: string | null = null;
const invoke = mock(async (command: string, _args?: unknown): Promise<unknown> => {
  if (command === failure) throw new Error('disk unavailable');
  switch (command) {
    case 'template_assist_get_settings': return settings;
    case 'template_assist_save_settings': return undefined;
    case 'api_template_meeting_info': return { templateId: 'one_on_one', hasEdits: true, transcriptTokens: 0 };
    case 'api_get_template': return base;
    case 'api_template_revise': return { base, template: { ...base, style: 'Concise' }, changes: [{ field: 'style', note: 'Shorter notes' }], warnings: [] };
    case 'api_save_template': return undefined;
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

type PanelProps = ComponentProps<typeof SummaryPanel>;
let renderer: ReactTestRenderer;
const text = (n: ReactTestInstance): string => n.children.map(c => typeof c === 'string' ? c : text(c)).join('');
const buttons = (label: string) => renderer.root.findAll(n => n.type === 'button' && text(n) === label);
async function click(label: string) { await act(async () => { await buttons(label)[0].props.onClick(); }); }
async function mount(overrides: Partial<PanelProps> = {}) {
  await act(async () => {
    renderer = create(<SummaryPanel
      meeting={{ id: 'meeting-a', title: 'Meeting A', created_at: '2026-10-14T00:00:00Z' }}
      meetingTitle="Meeting A" isSummaryDirty={false} summaryRef={{ current: null }} isSaving={false}
      onSaveAll={async () => true} onCopySummary={async () => {}} aiSummary={{ markdown: 'Existing summary' }}
      summaryStatus="completed" transcripts={[]}
      modelConfig={{ provider: 'ollama', model: 'test', whisperModel: 'base' }}
      setModelConfig={() => {}} onSaveModelConfig={async () => {}} onGenerateSummary={async () => {}}
      onStopGeneration={() => {}} customPrompt="" onSaveSummary={async () => {}}
      onSummaryChange={() => {}} onDirtyChange={() => {}} summaryError={null}
      onRegenerateSummary={regenerate} getSummaryStatusMessage={() => ''}
      availableTemplates={[{ id: 'one_on_one', name: 'One on one' }]}
      selectedTemplate="one_on_one" currentTemplate={{ id: 'one_on_one', name: 'One on one', source: null }}
      languageFallback={false} onTemplateSelect={() => {}} {...overrides}
    />);
  });
}
async function update(overrides: Partial<PanelProps>) {
  const props = renderer.root.findByType(SummaryPanel).props as PanelProps;
  await act(async () => { renderer.update(<SummaryPanel {...props} {...overrides} />); });
}
async function saveTemplate(overrides: Partial<PanelProps> = {}) {
  await mount(overrides);
  await click('Improve template…'); await click('Generate');
  await click('Accept selected'); await click('Save template');
  expect(invoke.mock.calls.filter(([c]) => c === 'api_save_template')).toHaveLength(1);
}
beforeEach(() => { invoke.mockClear(); regenerate.mockClear(); });
afterEach(() => { act(() => renderer?.unmount()); });
for (const status of ['processing', 'summarizing', 'regenerating'] as const) {
  test(`improve action is hidden while ${status}`, async () => {
    await mount({ summaryStatus: status });
    expect(buttons('Improve template…')).toHaveLength(0);
  });
}
test('improve action is hidden when the current template is missing from the list', async () => {
  await mount({ availableTemplates: [{ id: 'other', name: 'Other' }] });
  expect(buttons('Improve template…')).toHaveLength(0);
});
test('improve action is shown for a completed summary with an available template', async () => {
  await mount(); expect(buttons('Improve template…')).toHaveLength(1);
});
test('Regenerate closes the saved dialog and it stays closed after regeneration completes', async () => {
  await saveTemplate();
  const onClick = buttons('Regenerate')[0].props.onClick;
  await act(async () => { onClick(); });
  expect(regenerate).toHaveBeenCalledWith('one_on_one');
  expect(renderer.root.findAllByType('textarea')).toHaveLength(0);
  expect(buttons('Regenerate')).toHaveLength(0);
  await update({ summaryStatus: 'regenerating' });
  await update({ summaryStatus: 'completed' });
  expect(renderer.root.findAllByType('textarea')).toHaveLength(0);
  expect(buttons('Regenerate')).toHaveLength(0);
  expect(invoke.mock.calls.filter(([c]) => c === 'api_template_meeting_info')).toHaveLength(1);
  expect(regenerate).toHaveBeenCalledTimes(1);
});
test('a second Regenerate click before status changes does not start another regeneration', async () => {
  await saveTemplate();
  const onClick = buttons('Regenerate')[0].props.onClick;
  await act(async () => { onClick(); onClick(); });
  expect(regenerate).toHaveBeenCalledTimes(1);
  expect(regenerate).toHaveBeenCalledWith('one_on_one');
});
test('declining the unsaved-edits gate permits a later Regenerate click', async () => {
  const confirm = spyOn(window, 'confirm').mockReturnValue(false);
  try {
    await saveTemplate({ isSummaryDirty: true }); await click('Regenerate');
    expect(regenerate).not.toHaveBeenCalled();
    expect(buttons('Regenerate')).toHaveLength(1);
    confirm.mockReturnValue(true);
    await click('Regenerate');
    expect(regenerate).toHaveBeenCalledTimes(1);
    expect(buttons('Regenerate')).toHaveLength(0);
  } finally { confirm.mockRestore(); }
});
