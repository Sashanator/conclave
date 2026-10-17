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

const first: TranscriptSegmentData = { id: 'a', timestamp: 1, text: 'Short line.' };
const second: TranscriptSegmentData = { id: 'b', timestamp: 2, text: 'Second line here.' };

let renderer: ReactTestRenderer | undefined;
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
});

function view(getSpeakerLabel?: (s: TranscriptSegmentData) => string | undefined) {
  return <VirtualizedTranscriptView segments={[first, second]} getSpeakerLabel={getSpeakerLabel} />;
}
async function render(element: ReturnType<typeof view>) {
  await act(async () => { renderer = create(element); });
}
type HostNode = { type: string; props: Record<string, any>; children: (HostNode | string)[] | null };
// Host-only tree: the timestamp row (flex container) of one line
function rowOf(id: string): HostNode {
  const find = (node: HostNode | string): HostNode | undefined => {
    if (typeof node === 'string' || !node.children) return undefined;
    if (node.props.id === `segment-${id}`) return node.children[0] as HostNode;
    for (const child of node.children) {
      const hit = find(child);
      if (hit) return hit;
    }
    return undefined;
  };
  const row = find(renderer!.toJSON() as HostNode);
  if (!row) throw new Error(`no row for ${id}`);
  return row;
}
function labels(): ReactTestInstance[] {
  return renderer!.root.findAll((n) => n.props['data-speaker-label'] !== undefined);
}

describe('live speaker label', () => {
  test('a line with a label renders a small label element before its text', async () => {
    await render(view((s) => (s.id === 'a' ? 'You' : 'Others')));
    const label = labels().find((n) => n.props['data-speaker-label'] === 'You');
    expect(label).toBeDefined();
    expect(label!.type).toBe('span');
    expect(label!.props.className).toContain('text-xs');
    expect(label!.children).toEqual(['You']);
    const children = rowOf('a').children as HostNode[];
    const labelAt = children.findIndex((n) => n.props['data-speaker-label'] === 'You');
    const textAt = children.findIndex((n) => n.props.className === 'flex-1');
    expect(labelAt).toBeGreaterThanOrEqual(0);
    expect(labelAt).toBeLessThan(textAt);
    expect(labels().map((n) => n.props['data-speaker-label'])).toEqual(['You', 'Others']);
  });

  test('without a label, rows keep the timestamp and text columns only', async () => {
    await render(view());
    expect(labels().length).toBe(0);
    expect((rowOf('a').children as HostNode[]).map((n) => n.props.className)).toEqual([
      'text-xs text-muted-foreground mt-1 flex-shrink-0 min-w-[50px]',
      'flex-1',
    ]);
  });
});
