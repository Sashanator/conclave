import { describe, expect, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { SearchHits } from '../../src/components/Sidebar/SearchHits';
import type { SearchHit } from '../../src/lib/meeting-target';
import type { ReactTestInstance } from 'react-test-renderer';

const hit = (over: Partial<SearchHit>): SearchHit => ({
  meetingId: 'm1', title: 'Standup', source: 'transcript', snippet: 'план', transcriptId: 't9', audioStart: 4, ...over,
});

function render(hits: SearchHit[], onOpen: (href: string) => void): ReactTestRenderer {
  let r!: ReactTestRenderer;
  act(() => {
    r = create(<SearchHits hits={hits} onOpen={onOpen} />);
  });
  return r;
}

// react-test-renderer does not bubble events: walk up the instance tree the way the DOM would
function click(target: ReactTestInstance) {
  let stopped = false;
  const event = { stopPropagation: () => { stopped = true; } };
  for (let node: ReactTestInstance | null = target; node && !stopped; node = node.parent) {
    node.props.onClick?.(event);
  }
}

describe('SearchHits', () => {
  test('a hit click is not handled again by the row it sits in', () => {
    const opened: string[] = [];
    const rowClicks: number[] = [];
    let r!: ReactTestRenderer;
    act(() => {
      r = create(
        <div onClick={() => rowClicks.push(1)}>
          <SearchHits hits={[hit({})]} onOpen={(href) => opened.push(href)} />
        </div>,
      );
    });
    act(() => click(r.root.findByType('button')));
    expect(opened).toEqual(['/meeting-details?id=m1&line=t9']);
    expect(rowClicks).toEqual([]);
  });

  test('hits render a source label and the snippet text', () => {
    const r = render([hit({ source: 'summary', transcriptId: null, audioStart: null, snippet: 'бюджет' })], () => {});
    const text = JSON.stringify(r.toJSON());
    expect(text).toContain('Summary');
    expect(text).toContain('бюджет');
  });

  test('a transcript hit click opens /meeting-details?id=m1&line=t9', () => {
    const opened: string[] = [];
    const r = render([hit({})], (href) => opened.push(href));
    const button = r.root.findByType('button');
    act(() => button.props.onClick({ stopPropagation: () => {} }));
    expect(opened).toEqual(['/meeting-details?id=m1&line=t9']);
  });

  test('a snippet with markup renders literally', () => {
    const r = render([hit({ snippet: '<b>x</b>' })], () => {});
    expect(r.root.findAll((n) => n.type === 'b').length).toBe(0);
    expect(JSON.stringify(r.toJSON())).toContain('<b>x</b>');
  });
});
