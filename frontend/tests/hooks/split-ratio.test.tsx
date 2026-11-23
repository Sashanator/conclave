import { beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';
import { useSplitRatio } from '../../src/hooks/useSplitRatio';

let split: ReturnType<typeof useSplitRatio>;
function View({ offset }: { offset?: number }) {
  split = useSplitRatio({ storageKey: 'test.ratio', defaultRatio: 0.3, min: 0.2, max: 0.5, label: 'test', offset });
  return null;
}
const key = (k: string) => {
  const event = { key: k, preventDefault: mock(() => {}) };
  act(() => { split.separatorProps.onKeyDown(event as never); });
  return event;
};

beforeEach(() => localStorage.clear());

describe('useSplitRatio keyboard', () => {
  test('arrows step by 5 points, clamped to the range; Home and End jump to the ends', () => {
    act(() => { create(<View />); });
    key('ArrowRight');
    expect(split.ratio).toBeCloseTo(0.35);
    key('ArrowLeft');
    key('ArrowLeft');
    expect(split.ratio).toBeCloseTo(0.25);
    key('Home');
    expect(split.ratio).toBe(0.2);
    key('ArrowLeft');
    expect(split.ratio).toBe(0.2);
    key('End');
    expect(split.ratio).toBe(0.5);
    key('ArrowRight');
    expect(split.ratio).toBe(0.5);
    expect(localStorage.getItem('test.ratio')).toBe('0.5');
  });

  test('other keys are left alone', () => {
    act(() => { create(<View />); });
    const event = key('Tab');
    expect(event.preventDefault).not.toHaveBeenCalled();
    expect(split.ratio).toBe(0.3);
  });
});

describe('useSplitRatio offset', () => {
  test('a second splitter reads its width from the pointer minus the offset, and stops after pointer up', () => {
    act(() => { create(<View offset={0.5} />); });
    split.containerRef.current = { getBoundingClientRect: () => ({ left: 100, width: 1000 }) } as unknown as HTMLDivElement;
    const capture = { setPointerCapture: mock(() => {}) };
    act(() => {
      split.separatorProps.onPointerDown({ preventDefault: () => {}, currentTarget: capture, pointerId: 1 } as never);
    });
    // The boundary sits at 0.5 of the container, so pointer x 800 means a 0.2 width.
    act(() => { split.containerHandlers.onPointerMove({ clientX: 800 } as never); });
    expect(split.ratio).toBeCloseTo(0.2);
    act(() => { split.containerHandlers.onPointerUp(); });
    expect(localStorage.getItem('test.ratio')).toBe('0.2');
    act(() => { split.containerHandlers.onPointerMove({ clientX: 900 } as never); });
    expect(split.ratio).toBeCloseTo(0.2);
  });
});

describe('useSplitRatio gutter and px mode', () => {
  const drag = (x: number, rect: object) => {
    split.containerRef.current = { getBoundingClientRect: () => rect } as unknown as HTMLDivElement;
    act(() => { split.separatorProps.onPointerDown({ preventDefault: () => {}, currentTarget: { setPointerCapture: () => {} }, pointerId: 1 } as never); });
    act(() => { split.containerHandlers.onPointerMove({ clientX: x } as never); });
    act(() => { split.containerHandlers.onPointerUp(); });
  };
  let opts: Record<string, unknown> = {};
  function PxView() {
    split = useSplitRatio({ storageKey: 'test.px', defaultRatio: 200, min: 100, max: 400, label: 'test', ...opts } as never);
    return null;
  }

  test('a gutter keeps the handle centred on the pointer', () => {
    opts = {};
    act(() => { create(<View />); });
    // Re-render with a gutter of 32: pointer at the handle centre of a 0.4 split of (1032 - 32) wide.
    function G() { split = useSplitRatio({ storageKey: 'test.g', defaultRatio: 0.3, min: 0.2, max: 0.5, label: 't', gutter: 32 }); return null; }
    act(() => { create(<G />); });
    drag(400 + 16, { left: 0, width: 1032, right: 1032 });
    expect(split.ratio).toBeCloseTo(0.4);
  });

  test('px mode from the start edge subtracts the offset and clamps', () => {
    opts = { px: 'start', offset: 16 };
    act(() => { create(<PxView />); });
    drag(316, { left: 0, width: 1000, right: 1000 });
    expect(split.ratio).toBe(300);
    expect(localStorage.getItem('test.px')).toBe('300');
    drag(900, { left: 0, width: 1000, right: 1000 });
    expect(split.ratio).toBe(400);
  });

  test('px mode from the end edge measures from the right, and arrows are mirrored', () => {
    opts = { px: 'end' };
    act(() => { create(<PxView />); });
    drag(700, { left: 0, width: 1000, right: 1000 });
    expect(split.ratio).toBe(300);
    key('ArrowLeft');
    expect(split.ratio).toBe(316);
    expect(split.separatorProps['aria-valuetext']).toBe('test 316 pixels');
  });
});
