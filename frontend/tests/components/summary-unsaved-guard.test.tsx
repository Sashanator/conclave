import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { createRef } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import { confirmLeave, setUnsavedChanges } from '../../src/lib/unsaved-guard';
import type { BlockNoteSummaryViewRef } from '../../src/components/AISummary/BlockNoteSummaryView';

const originals = {
  blockNoteReact: { ...await import('@blocknote/react') },
  blockNoteShadcn: { ...await import('@blocknote/shadcn') },
  theme: { ...await import('../../src/contexts/ThemeContext') },
  markdown: { ...await import('../../src/lib/blocknote-markdown') },
  dynamic: { ...await import('next/dynamic') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@blocknote/react', () => originals.blockNoteReact);
  mock.module('@blocknote/shadcn', () => originals.blockNoteShadcn);
  mock.module('../../src/contexts/ThemeContext', () => originals.theme);
  mock.module('../../src/lib/blocknote-markdown', () => originals.markdown);
  mock.module('next/dynamic', () => originals.dynamic);
});

const editor = { document: [], tryParseMarkdownToBlocks: async () => [], replaceBlocks: () => {} };
mock.module('@blocknote/react', () => ({ useCreateBlockNote: () => editor }));
mock.module('@blocknote/shadcn', () => ({ BlockNoteView: () => null }));
mock.module('../../src/contexts/ThemeContext', () => ({ useTheme: () => ({ resolved: 'light' }) }));
mock.module('../../src/lib/blocknote-markdown', () => ({ blocksToMarkdownSafely: async () => ({ markdown: '# Notes' }) }));
// The lazily loaded editor is replaced by a stub that the test drives through its onChange prop.
mock.module('next/dynamic', () => ({ default: () => (() => null) }));
const { BlockNoteSummaryView } = await import('../../src/components/AISummary/BlockNoteSummaryView');

const realConfirm = window.confirm;
const block = { id: 'b1', type: 'paragraph', props: {}, content: [{ type: 'text', text: 'Action', styles: {} }], children: [] };
const summary = { summary_json: [block] };

let renderer: ReactTestRenderer | undefined;
let onSave: ReturnType<typeof mock>;
// Mirrors the unsaved guard: confirmLeave() asks only while the flag is set, and the stub answers "stay" (false).
function isDirty(): boolean {
  let asked = false;
  window.confirm = () => { asked = true; return false; };
  const leaveAllowed = confirmLeave();
  window.confirm = realConfirm;
  return asked && !leaveAllowed;
}
function editorNode(): ReactTestInstance {
  return renderer!.root.findAll((n) => typeof n.props.onChange === 'function' && n.props.initialContent !== undefined)[0];
}
async function wait(ms: number) {
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, ms)); });
}
async function edit() {
  await wait(150); // the view ignores edits until its content-loaded delay has passed
  await act(async () => { editorNode().props.onChange([block]); });
}

beforeEach(() => {
  onSave = mock(async () => {});
  setUnsavedChanges(false);
});
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  setUnsavedChanges(false);
});

describe('summary edits and the app-wide unsaved guard', () => {
  test('an edit marks the guard dirty', async () => {
    await act(async () => { renderer = create(<BlockNoteSummaryView summaryData={summary} onSave={onSave} />); });
    expect(isDirty()).toBe(false);
    await edit();
    expect(isDirty()).toBe(true);
  });

  test('unmounting clears the guard', async () => {
    await act(async () => { renderer = create(<BlockNoteSummaryView summaryData={summary} onSave={onSave} />); });
    await edit();
    await act(async () => renderer!.unmount());
    renderer = undefined;
    expect(isDirty()).toBe(false);
  });

  test('a successful save clears the guard', async () => {
    const ref = createRef<BlockNoteSummaryViewRef>();
    await act(async () => { renderer = create(<BlockNoteSummaryView ref={ref} summaryData={summary} onSave={onSave} />); });
    await edit();
    expect(isDirty()).toBe(true);
    await act(async () => { await ref.current!.saveSummary(); });
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(isDirty()).toBe(false);
  });
});
