import { afterAll, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { forwardRef } from 'react';
import type { MeetingSummary, Transcript } from '../../src/types';

const originals = {
  blockNote: { ...await import('../../src/components/AISummary/BlockNoteSummaryView') },
  empty: { ...await import('../../src/components/EmptyStateSummary') },
  core: { ...await import('@tauri-apps/api/core') },
};
afterAll(() => {
  mock.module('../../src/components/AISummary/BlockNoteSummaryView', () => originals.blockNote);
  mock.module('../../src/components/EmptyStateSummary', () => originals.empty);
  mock.module('@tauri-apps/api/core', () => originals.core);
});
mock.module('../../src/components/AISummary/BlockNoteSummaryView', () => ({ BlockNoteSummaryView: forwardRef(() => null) }));
mock.module('../../src/components/EmptyStateSummary', () => ({ EmptyStateSummary: () => null }));
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke: async () => null }));

const { SummaryGeneratorButtonGroup } = await import('../../src/components/MeetingDetails/SummaryGeneratorButtonGroup');
const { SummaryUpdaterButtonGroup } = await import('../../src/components/MeetingDetails/SummaryUpdaterButtonGroup');
const { SummaryPanel } = await import('../../src/components/MeetingDetails/SummaryPanel');
const { Button } = await import('../../src/components/ui/button');

const text = (n: { children: unknown[] }): string => n.children.map((c) => (typeof c === 'string' ? c : text(c as { children: unknown[] }))).join('');
async function mount(el: React.ReactElement) {
  let r!: ReactTestRenderer;
  await act(async () => { r = create(el); });
  return r;
}

describe('summary toolbar', () => {
  test('Regenerate is the one primary button', async () => {
    const r = await mount(
      <SummaryGeneratorButtonGroup
        modelConfig={{ provider: 'ollama', model: 'm', whisperModel: 'base' }} setModelConfig={() => {}} onSaveModelConfig={async () => {}}
        onGenerateSummary={async () => {}} onStopGeneration={() => {}} customPrompt="" summaryStatus="completed"
        availableTemplates={[]} selectedTemplate={null} currentTemplate={null} onTemplateSelect={() => {}} hasSummary
      />,
    );
    const primary = r.root.findAllByType(Button).filter((b) => (b.props.variant ?? 'default') === 'default');
    expect(primary.length).toBe(1);
    expect(text(primary[0])).toContain('Regenerate');
  });

  test('a dirty Save has no green class', async () => {
    const r = await mount(<SummaryUpdaterButtonGroup isSaving={false} isDirty onSave={async () => {}} onCopy={async () => {}} />);
    const save = r.root.findAllByType(Button)[0];
    expect(String(save.props.className ?? '')).not.toContain('bg-green');
    expect(save.props.className).toContain('border-primary');
  });

  const panel = (status: 'completed' | 'error') => (
    <SummaryPanel
      meeting={{ id: 'm', title: 'M', created_at: '2026-10-14T00:00:00Z' }} meetingTitle="M" isSummaryDirty={false} summaryRef={{ current: null }}
      isSaving={false} onSaveAll={async () => false} onCopySummary={async () => {}} aiSummary={{ markdown: 'x' } as MeetingSummary}
      summaryStatus={status} transcripts={[{ text: 't' } as Transcript]} modelConfig={{ provider: 'ollama', model: 'm', whisperModel: 'base' }}
      setModelConfig={() => {}} onSaveModelConfig={async () => {}} onGenerateSummary={async () => {}} onStopGeneration={() => {}} customPrompt=""
      onSaveSummary={async () => {}} onSummaryChange={() => {}} onDirtyChange={() => {}} summaryError={null} onRegenerateSummary={async () => {}}
      getSummaryStatusMessage={(s) => (s === 'error' ? 'Summary failed' : 'Summary completed')} availableTemplates={[]} selectedTemplate={null}
      currentTemplate={null} languageFallback={false} onTemplateSelect={() => {}}
    />
  );

  test('no green completed bar; an error still shows its message', async () => {
    const done = await mount(panel('completed'));
    expect(text(done.root)).not.toContain('Summary completed');
    const err = await mount(panel('error'));
    expect(text(err.root)).toContain('Summary failed');
  });
});
