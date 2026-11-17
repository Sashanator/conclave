import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  sonner: { ...await import('sonner') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
});

const BRIEF = {
  eventTitle: '1:1 Ivan',
  eventStart: '2026-10-26T11:00:00Z',
  templateName: '1:1',
  previousMeetingId: 'm-old',
  previousTitle: '<img src=x> Ivan sync',
  previousDate: '2026-10-19T10:00:00Z',
  requests: ['<b>Send the deck</b>'],
  decisions: ['Ship the plan'],
  openTasks: [],
};

let state: { isCall: boolean; briefId: string | null } = { isCall: true, briefId: 'b1' };
let regenerate: () => Promise<unknown> = async () => 'b2';
const calls: Array<[string, unknown]> = [];
const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  calls.push([command, args]);
  if (command === 'brief_for_meeting') return { ...state };
  if (command === 'brief_get') return structuredClone(BRIEF);
  if (command === 'brief_regenerate') return regenerate();
  throw new Error(`Unexpected command: ${command}`);
});
const toastError = mock((_m: string) => {});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('sonner', () => ({ ...originals.sonner, toast: { error: toastError, success: () => {} } }));

const { MeetingBrief } = await import('../../src/components/Briefs/MeetingBrief');

let renderer: ReactTestRenderer | null = null;
const textOf = (n: ReactTestInstance | string): string =>
  typeof n === 'string' ? n : n.children.map(textOf).join('');
const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
async function mount() {
  renderer?.unmount();
  await act(async () => { renderer = create(<MeetingBrief meetingId="m1" />); });
  await flush();
}
const regenButton = () =>
  renderer!.root.findAll((n) => n.type === 'button' && textOf(n).includes('Regenerate brief'))[0];

beforeEach(() => {
  state = { isCall: true, briefId: 'b1' };
  regenerate = async () => 'b2';
  calls.length = 0;
  toastError.mockClear();
});

describe('MeetingBrief', () => {
  test('renders nothing for a meeting that is not a call', async () => {
    state = { isCall: false, briefId: null };
    await mount();
    expect(renderer!.toJSON()).toBeNull();
  });

  test('shows the brief when there is one, as literal text', async () => {
    await mount();
    expect(calls).toContainEqual(['brief_for_meeting', { meetingId: 'm1' }]);
    expect(calls).toContainEqual(['brief_get', { id: 'b1' }]);
    expect(renderer!.root.findAll((n) => n.type === 'img')).toHaveLength(0);
    expect(JSON.stringify(renderer!.toJSON())).toContain('<img src=x> Ivan sync');
  });

  test('says so when there is no brief yet', async () => {
    state = { isCall: true, briefId: null };
    await mount();
    expect(textOf(renderer!.root)).toContain('No brief yet');
  });

  test('regenerate reloads the brief', async () => {
    await mount();
    await act(async () => { await regenButton().props.onClick(); });
    await flush();
    expect(calls).toContainEqual(['brief_regenerate', { meetingId: 'm1' }]);
    expect(calls.filter(([c]) => c === 'brief_for_meeting')).toHaveLength(2);
    expect(toastError).not.toHaveBeenCalled();
  });

  test('a regenerate error shows its text and keeps the old brief', async () => {
    regenerate = async () => { throw 'No template matches this title'; };
    await mount();
    await act(async () => { await regenButton().props.onClick(); });
    await flush();
    expect(toastError).toHaveBeenCalledWith('No template matches this title');
    expect(calls.filter(([c]) => c === 'brief_for_meeting')).toHaveLength(1);
    expect(JSON.stringify(renderer!.toJSON())).toContain('Ivan sync');
  });
});
