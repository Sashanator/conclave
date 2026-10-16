import { describe, expect, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { MeetingDetailsSplitView } from '../../src/components/MeetingDetails/MeetingDetailsSplitView';

function render(props: Partial<Parameters<typeof MeetingDetailsSplitView>[0]> = {}): ReactTestRenderer {
  let r!: ReactTestRenderer;
  act(() => {
    r = create(
      <MeetingDetailsSplitView
        transcript={<p>TRANSCRIPT</p>}
        notes={<p>NOTES</p>}
        notesCount={1}
        summary={<p>SUMMARY</p>}
        layout="three-columns"
        activeTab="transcript"
        onTabChange={() => {}}
        {...props}
      />,
    );
  });
  return r;
}
const text = (r: ReactTestRenderer) => JSON.stringify(r.toJSON());
const count = (s: string, needle: string) => s.split(needle).length - 1;
// Host divs only: the SplitHandle component also carries role and aria-label props.
const separators = (r: ReactTestRenderer) =>
  r.root.findAll((n) => n.type === 'div' && n.props.role === 'separator').map((n) => n.props['aria-label']);

describe('MeetingDetailsSplitView layouts', () => {
  test('three columns render the notes once, with a splitter on each side of it', () => {
    const r = render();
    expect(count(text(r), 'NOTES')).toBe(1);
    expect(separators(r)).toEqual(['Resize transcript', 'Resize notes']);
  });

  test('a collapsed notes column keeps the notes but drops its splitter', () => {
    const r = render({ notesCollapsed: true });
    expect(count(text(r), 'NOTES')).toBe(1);
    expect(separators(r)).toEqual(['Resize transcript']);
  });

  test('sub-tab layout renders the notes once, inside the left pane, with one splitter', () => {
    const r = render({ layout: 'subtab' });
    const s = text(r);
    expect(count(s, 'NOTES')).toBe(1);
    expect(count(s, 'TRANSCRIPT')).toBe(1);
    expect(s).toContain('"My notes ("');
    expect(separators(r)).toEqual(['Resize transcript and summary']);
  });
});
