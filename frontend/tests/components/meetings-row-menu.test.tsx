import { describe, expect, mock, test } from 'bun:test';
import { createRoot } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import type { MeetingRow } from '../../src/lib/meetings';

const { RowMenu } = await import('../../src/components/MeetingsTable/RowMenu');

const row: MeetingRow = {
  id: 'm1', title: 'Weekly', titleSource: 'user', templateId: null, emoji: null, reviewed: false, gist: '',
  startedAt: null, endedAt: null, createdAt: '2026-11-16 10:00:00+00:00', calendarUid: null,
  calendarStart: null, calendarEnd: null, calendarCancelledAt: null, hasTranscript: false, hasSummary: false, prepPoints: 0, hasPrep: false,
};

// The real Radix menu: it opens from the keyboard and renders into a portal on document.body.
async function open(live: boolean, onDelete: () => void, onOpen: () => void = () => {}) {
  const host = document.createElement('div');
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => { root.render(<RowMenu row={row} live={live} onDelete={onDelete} onOpen={onOpen} />); });
  const trigger = host.querySelector('button[aria-label="Actions: Weekly"]')!;
  await act(async () => { trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })); });
  const items = () => Array.from(document.querySelectorAll('[role="menuitem"]')) as HTMLElement[];
  return { items, close: async () => { await act(async () => { root.unmount(); }); host.remove(); } };
}

describe('row menu', () => {
  test('Open tells the caller', async () => {
    const onOpen = mock(() => {});
    const m = await open(false, () => {}, onOpen);
    await act(async () => { m.items().find((i) => i.textContent === 'Open')!.click(); });
    expect(onOpen).toHaveBeenCalledTimes(1);
    await m.close();
  });

  test('Delete meeting… asks the caller to confirm', async () => {
    const onDelete = mock(() => {});
    const m = await open(false, onDelete);
    const del = m.items().find((i) => i.textContent === 'Delete meeting…')!;
    expect(del.getAttribute('aria-disabled')).toBeNull();
    await act(async () => { del.click(); });
    expect(onDelete).toHaveBeenCalledTimes(1);
    await m.close();
  });

  test('Delete meeting… is disabled on the live row', async () => {
    const onDelete = mock(() => {});
    const m = await open(true, onDelete);
    const del = m.items().find((i) => i.textContent === 'Delete meeting…')!;
    expect(del.getAttribute('aria-disabled')).toBe('true');
    await act(async () => { del.click(); });
    expect(onDelete).not.toHaveBeenCalled();
    await m.close();
  });
});
