import { afterAll, afterEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { TranscriptSegmentData } from '../../src/types';

const originalPopover = { ...await import('../../src/components/ui/popover') };
const originalTooltip = { ...await import('../../src/components/ui/tooltip') };
const originalStatusBar = { ...await import('../../src/components/RecordingStatusBar') };
afterAll(() => {
  mock.module('../../src/components/ui/popover', () => originalPopover);
  mock.module('../../src/components/ui/tooltip', () => originalTooltip);
  mock.module('../../src/components/RecordingStatusBar', () => originalStatusBar);
});
const pass = ({ children }: { children?: ReactNode }) => <>{children}</>;
mock.module('../../src/components/ui/popover', () => ({
  Popover: pass, PopoverAnchor: pass, PopoverTrigger: pass, PopoverContent: pass,
}));
mock.module('../../src/components/ui/tooltip', () => ({ Tooltip: pass, TooltipTrigger: pass, TooltipContent: pass }));
mock.module('../../src/components/RecordingStatusBar', () => ({ RecordingStatusBar: () => null }));
const { VirtualizedTranscriptView } = await import('../../src/components/VirtualizedTranscriptView');

// Longer than the 800 ms reveal, so the typewriter has finished.
const REVEAL_DONE_MS = 1000;
const first: TranscriptSegmentData = { id: 'a', timestamp: 1, text: 'Short line.' };
const newest = (mark: TranscriptSegmentData['mark'] = null): TranscriptSegmentData => ({
  id: 'b', timestamp: 2, text: 'The newest line is long enough to need the typewriter effect.', mark,
});

let renderer: ReactTestRenderer | undefined;
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
});

function view(segments: TranscriptSegmentData[]) {
  return <VirtualizedTranscriptView segments={segments} isRecording enableStreaming onMarkChange={() => {}} />;
}
async function wait(ms: number) {
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, ms)); });
}
function markControl(text: string): ReactTestInstance | undefined {
  return renderer!.root.findAll((n) => n.type === 'div' && n.props.role === 'button' && String(n.props['aria-label']).startsWith('Mark line: '))
    .find((n) => String(n.props['aria-label']).includes(text));
}
function lineText(node: ReactTestInstance): string {
  return node.children.map((c) => (typeof c === 'string' ? c : lineText(c))).join('');
}

describe('newest transcript line while recording', () => {
  test('after the reveal completes, the newest line has its mark control and full text', async () => {
    await act(async () => { renderer = create(view([first, newest()])); });
    await wait(REVEAL_DONE_MS);
    expect(markControl('The newest line is long enough')).toBeDefined();
    expect(JSON.stringify(renderer!.toJSON())).toContain('The newest line is long enough to need the typewriter effect.');
  });

  test('marking a line during the reveal finishes the newest line instead of leaving it truncated', async () => {
    await act(async () => { renderer = create(view([first, newest()])); });
    // A mark on another line re-runs the streaming effect for the same newest id while its reveal is running.
    await act(async () => { renderer!.update(view([{ ...first, mark: 'important' }, newest()])); });
    await wait(REVEAL_DONE_MS);
    expect(markControl('The newest line is long enough')).toBeDefined();
    expect(JSON.stringify(renderer!.toJSON())).toContain('The newest line is long enough to need the typewriter effect.');
  });
});
