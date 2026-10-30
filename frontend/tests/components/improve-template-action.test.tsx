import { afterAll, afterEach, beforeEach, expect, mock, test } from 'bun:test';
import { useState, type ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { TemplateJson } from '../../src/lib/template-draft';
import { readSummaryMetadata } from '../../src/lib/summary-content';
const originals = { core: { ...await import('@tauri-apps/api/core') }, dialog: { ...await import('../../src/components/ui/dialog') } };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('../../src/components/ui/dialog', () => originals.dialog);
});
const pass = ({ children }: { children?: ReactNode }) => <>{children}</>;
mock.module('../../src/components/ui/dialog', () => ({
  Dialog: ({ open, children }: { open: boolean; children?: ReactNode }) => open ? <>{children}</> : null,
  DialogContent: pass, DialogHeader: pass, DialogTitle: pass, DialogDescription: pass, DialogFooter: pass,
}));
const base: TemplateJson = { name: 'Original', sections: [{ title: 'Notes', instruction: 'List notes' }] };
const merged = { ...base, style: 'Be concise' };
const openTemplate = { ...base, name: 'Open-time name' };
let saved: TemplateJson;
let hasEdits: boolean;
let failure: string | null;
let reads: number;
const proposal = { base, template: merged, changes: [{ field: 'style', note: 'Shorter notes' }], warnings: [] };
const invoke = mock(async (command: string, _args?: unknown): Promise<unknown> => {
  if (failure === command) throw new Error('disk unavailable');
  switch (command) {
    case 'api_template_meeting_info': return { templateId: 'one_on_one', hasEdits, transcriptTokens: 1234 };
    case 'api_get_template': return ++reads === 1 ? openTemplate : saved;
    case 'template_assist_get_settings': return { includeTranscript: false, provider: null, model: null };
    case 'api_get_model_config': return undefined;
    case 'costs_get_prices': return [];
    case 'api_template_revise': return proposal;
    case 'api_save_template': return undefined;
    default: throw new Error(`Unexpected command: ${command}`);
  }
});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
const { ImproveTemplateAction } = await import('../../src/components/MeetingDetails/ImproveTemplateAction');
const regenerate = mock((_id: string) => {});
let renderer: ReactTestRenderer;
const text = (n: ReactTestInstance): string => n.children.map(c => typeof c === 'string' ? c : text(c)).join('');
const button = (label: string) => renderer.root.findAll(n => n.type === 'button' && text(n) === label)[0];
const checkbox = (label: string) => renderer.root.findAllByType('label').find(n => text(n) === label)!.findByType('input');
function Harness({ templateExists = true, templateId = 'one_on_one' }) {
  const [open, setOpen] = useState(false);
  return <ImproveTemplateAction meetingId="meeting-a" templateId={templateId} templateExists={templateExists}
    open={open} onOpenChange={setOpen} onRegenerate={regenerate} />;
}
async function mount(props = {}) { await act(async () => { renderer = create(<Harness {...props} />); }); }
async function click(label: string) { await act(async () => { await button(label).props.onClick(); }); }
async function review() {
  await mount(); await click('Improve template…');
  await act(async () => renderer.root.findByType('textarea').props.onChange({ target: { value: 'Make it shorter' } }));
  await click('Generate');
}
afterEach(() => { act(() => renderer?.unmount()); });
beforeEach(() => { saved = base; reads = 0; hasEdits = true; failure = null; invoke.mockClear(); regenerate.mockClear(); });
test('hidden for missing or empty template ids; legacy summaries have no template id', async () => {
  expect(readSummaryMetadata({ MeetingName: 'x' }).templateId).toBeNull();
  for (const props of [{ templateExists: false }, { templateId: '' }]) {
    await mount(props); expect(renderer.toJSON()).toBeNull(); act(() => renderer.unmount());
  }
  expect(invoke).not.toHaveBeenCalled();
});
test('no invoke on mount; meeting checkboxes, token label and edits-only generation', async () => {
  await mount(); expect(invoke).not.toHaveBeenCalled(); await click('Improve template…');
  expect(text(renderer.root)).toContain('What was wrong with this summary?');
  expect(checkbox('Your edits to this summary will be included').props.checked).toBe(true);
  expect(checkbox('Include the transcript (more accurate, costs more: ~1,234 tokens)').props.checked).toBe(false);
  expect(button('Generate').props.disabled).toBe(false); await click('Generate');
  expect(invoke).toHaveBeenCalledWith('api_template_revise', {
    templateId: 'one_on_one', request: '', meetingId: 'meeting-a', includeEdits: true, includeTranscript: false,
    previous: null, refinement: null,
  });
});
test('empty request without edits disables Generate', async () => {
  hasEdits = false; await mount(); await click('Improve template…');
  expect(button('Generate').props.disabled).toBe(true);
  expect(renderer.root.findAllByType('input')).toHaveLength(1);
});
test('Accept confirms changes, Back retains review, saves AI note and offers regeneration', async () => {
  await review(); await click('Accept selected');
  expect(text(renderer.root)).toContain('These changes will be saved:');
  expect(text(renderer.root)).not.toContain('Name');
  expect(invoke.mock.calls.filter(([c]) => c === 'api_get_template')).toHaveLength(1);
  await click('Back'); expect(button('Accept selected')).toBeDefined(); await click('Accept selected');
  await click('Save template');
  expect(invoke).toHaveBeenCalledWith('api_save_template', {
    templateId: 'one_on_one', template: merged, origin: { source: 'ai', note: 'Shorter notes' },
  });
  expect(text(renderer.root)).toContain('Regenerate this summary with the updated template');
  await click('Regenerate'); expect(regenerate).toHaveBeenCalledWith('one_on_one');
});
test('stale template blocks Save, with Back or Close and no persistence', async () => {
  await review(); await click('Accept selected'); saved = { ...base, style: 'Changed elsewhere' };
  await click('Save template');
  expect(text(renderer.root)).toContain('The template changed since this proposal was made.');
  expect(button('Save template').props.disabled).toBe(true);
  expect(button('Back')).toBeDefined(); expect(button('Close')).toBeDefined();
  expect(invoke.mock.calls.filter(([c]) => c === 'api_save_template')).toHaveLength(0);
});
test('load and save failures are inline', async () => {
  failure = 'api_template_meeting_info'; await mount(); await click('Improve template…');
  expect(text(renderer.root.findByProps({ role: 'alert' }))).toContain('disk unavailable');
  act(() => renderer.unmount()); failure = null; await review(); await click('Accept selected');
  failure = 'api_save_template'; await click('Save template');
  expect(text(renderer.root.findByProps({ role: 'alert' }))).toContain('disk unavailable');
  expect(regenerate).not.toHaveBeenCalled();
});
