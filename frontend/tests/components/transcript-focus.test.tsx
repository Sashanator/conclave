import { afterAll, describe, expect, mock, test } from 'bun:test';
import { useCallback, useRef, useState, useEffect, type ReactNode } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { TranscriptSegmentData } from '../../src/types';

const originalTooltip = { ...await import('../../src/components/ui/tooltip') };
const originalStatusBar = { ...await import('../../src/components/RecordingStatusBar') };
afterAll(() => {
  mock.module('../../src/components/ui/tooltip', () => originalTooltip);
  mock.module('../../src/components/RecordingStatusBar', () => originalStatusBar);
});
const pass = ({ children }: { children?: ReactNode }) => <>{children}</>;
mock.module('../../src/components/ui/tooltip', () => ({ Tooltip: pass, TooltipTrigger: pass, TooltipContent: pass }));
mock.module('../../src/components/RecordingStatusBar', () => ({ RecordingStatusBar: () => null }));
const { VirtualizedTranscriptView } = await import('../../src/components/VirtualizedTranscriptView');

const PAGE_ROWS = 3;
const PAGES = 3;
const FOCUS = 'seg-7'; // row 7 is only on page 3

// Mimics usePaginatedTranscripts: it drops a call made less than 100 ms after the last one.
function Harness({ onLoaded }: { onLoaded: (pages: number) => void }) {
  const [loaded, setLoaded] = useState(1);
  const [loadingMore, setLoadingMore] = useState(false);
  const loadedRef = useRef(1);
  const busyRef = useRef(false);
  const lastCallRef = useRef(0);
  const loadMore = useCallback(() => {
    if (Date.now() - lastCallRef.current < 100 || busyRef.current || loadedRef.current >= PAGES) return;
    lastCallRef.current = Date.now();
    busyRef.current = true;
    setLoadingMore(true);
    setTimeout(() => {
      loadedRef.current += 1;
      busyRef.current = false;
      setLoaded(loadedRef.current);
      setLoadingMore(false);
    }, 5);
  }, []);
  useEffect(() => onLoaded(loaded), [loaded, onLoaded]);
  const segments: TranscriptSegmentData[] = Array.from({ length: loaded * PAGE_ROWS }, (_, i) => ({
    id: `seg-${i}`, timestamp: i, text: `line ${i}`,
  }));
  return (
    <VirtualizedTranscriptView
      segments={segments}
      hasMore={loaded < PAGES}
      isLoadingMore={loadingMore}
      totalCount={PAGES * PAGE_ROWS}
      loadedCount={segments.length}
      onLoadMore={loadMore}
      focusSegmentId={FOCUS}
    />
  );
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

describe('VirtualizedTranscriptView focus', () => {
  test('a focus row on the third page is reached and flashed when pages load fast', async () => {
    let pages = 0;
    let r!: ReactTestRenderer;
    act(() => {
      r = create(<Harness onLoaded={(n) => { pages = n; }} />);
    });
    await sleep(800); // page loads are timers: let them run outside act, as the app does
    expect(pages).toBe(PAGES);
    expect(JSON.stringify(r.toJSON())).toContain('"data-focused":"true"');
    act(() => r.unmount());
  });
});
