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
