import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { Transcript } from '../../src/types';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  navigation: { ...await import('next/navigation') },
  recordingState: { ...await import('../../src/contexts/RecordingStateContext') },
  transcriptContext: { ...await import('../../src/contexts/TranscriptContext') },
  transcriptPanel: { ...await import('../../src/app/_components/TranscriptPanel') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('next/navigation', () => originals.navigation);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.recordingState);
  mock.module('../../src/contexts/TranscriptContext', () => originals.transcriptContext);
  mock.module('../../src/app/_components/TranscriptPanel', () => originals.transcriptPanel);
});

type Pending = { requestId: number; resolve(answer: string): void; reject(error: string): void };
let asks: Pending[] = [];
const invoke = mock(async (command: string, args?: Record<string, any>): Promise<unknown> => {
  if (command === 'copilot_get_settings') return { provider: null, model: null, answer_language: 'en', alert_words: [] };
  if (command === 'copilot_ask') {
    return new Promise((resolve, reject) => asks.push({ requestId: args!.requestId, resolve, reject }));
  }
  return undefined;
});
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));

const handlers = new Map<string, (event: { payload: unknown }) => void>();
mock.module('@tauri-apps/api/event', () => ({
  ...originals.event,
  listen: async (event: string, handler: (event: { payload: unknown }) => void) => {
    handlers.set(event, handler);
    return () => handlers.delete(event);
  },
}));

const push = mock((_path: string) => {});
mock.module('next/navigation', () => ({ ...originals.navigation, useRouter: () => ({ push }), usePathname: () => '/' }));

const recording = { isRecording: true, isStopping: false, isProcessing: false, isSaving: false, activeDuration: 125 as number | null };
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals.recordingState, useRecordingState: () => recording }));

const tx = {
  transcripts: [] as Transcript[],
  notes: [] as { id: string; at: number | null; text: string }[],
  addNote: mock((_text: string, _at: number | null) => {}),
  updateNote: mock(() => {}),
  deleteNote: mock(() => {}),
};
mock.module('../../src/contexts/TranscriptContext', () => ({
  ...originals.transcriptContext,
  useTranscripts: () => ({
    ...tx,
    transcriptsRef: { get current() { return tx.transcripts; } },
    notesRef: { get current() { return tx.notes; } },
  }),
}));
mock.module('../../src/app/_components/TranscriptPanel', () => ({ TranscriptPanel: () => null }));

const { CopilotProvider } = await import('../../src/contexts/CopilotContext');
const { CopilotPane } = await import('../../src/components/Copilot/CopilotPane');
const { LiveTranscriptAndNotes } = await import('../../src/app/_components/LiveTranscriptAndNotes');

const finalLine: Transcript = {
  id: '1-0', text: 'Бюджет на второй квартал', timestamp: '10:00:00', sequence_id: 1,
  audio_start_time: 100, audio_end_time: 104, is_partial: false,
};

const tree = () => (
  <CopilotProvider>
    <CopilotPane locked={!recording.isRecording} />
  </CopilotProvider>
);

function render(element: ReactNode = tree()) {
  let r!: ReactTestRenderer;
  act(() => { r = create(element as any); });
  return r;
}

const flush = () => act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
const textOf = (n: ReactTestInstance): string => n.children.map((c) => (typeof c === 'string' ? c : textOf(c))).join('');
function button(r: ReactTestRenderer, label: string): ReactTestInstance {
  const found = r.root.findAll((n) => n.type === 'button' && textOf(n).trim() === label);
  if (found.length !== 1) throw new Error(`expected one "${label}" button, found ${found.length}`);
  return found[0];
}
const hasButton = (r: ReactTestRenderer, label: string) => r.root.findAll((n) => n.type === 'button' && textOf(n).trim() === label).length > 0;
const card = (r: ReactTestRenderer, label: string) => r.root.findAll((n) => n.type === 'article' && n.props['aria-label'] === label);
const askBox = (r: ReactTestRenderer) => r.root.findByProps({ placeholder: 'Ask about this meeting…' });
const click = (r: ReactTestRenderer, label: string) => act(() => button(r, label).props.onClick());

beforeEach(() => {
  asks = [];
  invoke.mockClear();
  push.mockClear();
  tx.addNote.mockClear();
  tx.transcripts = [finalLine];
  tx.notes = [];
  Object.assign(recording, { isRecording: true, isStopping: false, isProcessing: false, isSaving: false, activeDuration: 125 });
  localStorage.clear();
});

describe('CopilotPane', () => {
  test('buttons_disabled_before_transcript', () => {
    tx.transcripts = [];
    const r = render();
    expect(button(r, 'Catch me up').props.disabled).toBe(true);
    expect(button(r, 'What should I ask?').props.disabled).toBe(true);
    expect(button(r, 'Explain the last minute').props.disabled).toBe(true);
    expect(JSON.stringify(r.toJSON())).toContain('Copilot starts once there is transcript');
  });

  test('ask_shows_pending_then_answer', async () => {
    const r = render();
    click(r, 'Catch me up');
    await flush();
    expect(invoke).toHaveBeenCalledWith('copilot_ask', expect.objectContaining({ requestId: expect.any(Number) }));
    expect(asks.length).toBe(1);
    expect((invoke.mock.calls.find((c) => c[0] === 'copilot_ask')![1] as any).request.action).toBe('catch_up');
    expect(card(r, 'Catch me up').length).toBe(1);
    expect(hasButton(r, 'Cancel')).toBe(true);
    expect(r.root.findAll((n) => n.props['aria-label'] === 'Loading').length).toBe(1);
    await act(async () => { asks[0].resolve('Они обсуждали бюджет.'); });
    await flush();
    expect(JSON.stringify(r.toJSON())).toContain('Они обсуждали бюджет.');
    expect(hasButton(r, 'Cancel')).toBe(false);
  });

  test('one_at_a_time', async () => {
    const r = render();
    click(r, 'Catch me up');
    await flush();
    expect(button(r, 'Catch me up').props.disabled).toBe(true);
    expect(button(r, 'What should I ask?').props.disabled).toBe(true);
    expect(button(r, 'Explain the last minute').props.disabled).toBe(true);
    expect(askBox(r).props.disabled).toBe(true);
  });

  test('cancel_removes_card', async () => {
    const r = render();
    click(r, 'Catch me up');
    await flush();
    const requestId = asks[0].requestId;
    click(r, 'Cancel');
    await flush();
    expect(invoke).toHaveBeenCalledWith('copilot_cancel', { requestId });
    expect(card(r, 'Catch me up').length).toBe(0);
    await act(async () => { asks[0].resolve('late answer'); });
    await flush();
    expect(JSON.stringify(r.toJSON())).not.toContain('late answer');
    expect(card(r, 'Catch me up').length).toBe(0);
  });

  test('error_shows_retry_and_settings', async () => {
    const r = render();
    click(r, 'Catch me up');
    await flush();
    await act(async () => { asks[0].reject('No API key'); });
    await flush();
    expect(JSON.stringify(r.toJSON())).toContain('No API key');
    expect(hasButton(r, 'Retry')).toBe(true);
    click(r, 'Copilot settings');
    expect(push).toHaveBeenCalledWith('/settings?tab=copilot');
    click(r, 'Retry');
    await flush();
    expect(asks.length).toBe(2);
    expect(card(r, 'Catch me up').length).toBe(1);
  });

  test('error_named_cancelled_is_not_shown', async () => {
    const r = render();
    click(r, 'Catch me up');
    await flush();
    await act(async () => { asks[0].reject('Cancelled'); });
    await flush();
    expect(card(r, 'Catch me up').length).toBe(0);
    expect(JSON.stringify(r.toJSON())).not.toContain('Cancelled');
  });

  test('add_to_notes', async () => {
    const r = render();
    click(r, 'Catch me up');
    await flush();
    await act(async () => { asks[0].resolve('Бюджет утвердили.'); });
    await flush();
    click(r, 'Add to notes');
    expect(tx.addNote).toHaveBeenCalledWith('Copilot (Catch me up): Бюджет утвердили.', 125);
    expect(hasButton(r, 'Added')).toBe(true);
  });

  test('answer_rendered_as_text', async () => {
    const r = render();
    click(r, 'Catch me up');
    await flush();
    await act(async () => { asks[0].resolve('<img src=x onerror=alert(1)>'); });
    await flush();
    expect(JSON.stringify(r.toJSON())).toContain('<img src=x onerror=alert(1)>');
    expect(r.root.findAll((n) => n.type === 'img').length).toBe(0);
  });

  test('stop_clears_cards_and_cancels', async () => {
    const r = render();
    click(r, 'Catch me up');
    await flush();
    const requestId = asks[0].requestId;
    recording.isRecording = false;
    await act(async () => { r.update(tree()); });
    await flush();
    expect(invoke).toHaveBeenCalledWith('copilot_cancel', { requestId });
    expect(card(r, 'Catch me up').length).toBe(0);
    expect(hasButton(r, 'Cancel')).toBe(false);
    await act(async () => { asks[0].resolve('after stop'); });
    await flush();
    expect(JSON.stringify(r.toJSON())).not.toContain('after stop');
    // The next recording's first ask is not refused as already running
    recording.isRecording = true;
    await act(async () => { r.update(tree()); });
    click(r, 'Catch me up');
    await flush();
    expect(asks.length).toBe(2);
  });
});

describe('Live right column tabs', () => {
  const side = () => (
    <CopilotProvider>
      <LiveTranscriptAndNotes isProcessingStop={false} isStopping={false} showModal={() => {}} />
    </CopilotProvider>
  );
  const tab = (r: ReactTestRenderer, label: string) => r.root.findAll((n) => n.props.role === 'tab' && textOf(n).trim() === label)[0];

  test('tab_persists', async () => {
    const r = render(side());
    await flush();
    act(() => tab(r, 'Copilot').props.onClick());
    expect(localStorage.getItem('conclave.liveSideTab')).toBe('copilot');
    expect(tab(r, 'Copilot').props['aria-selected']).toBe(true);
    act(() => r.unmount());
    const again = render(side());
    await flush();
    expect(tab(again, 'Copilot').props['aria-selected']).toBe(true);
    expect(askBox(again).props.disabled).toBe(false);
  });

  test('copilot_open_selects_copilot_tab_and_expands', async () => {
    localStorage.setItem('conclave.notesPaneCollapsed', '1');
    const r = render(side());
    await flush();
    expect(r.root.findAll((n) => n.props['aria-label'] === 'Show notes').length).toBe(1);
    await act(async () => { handlers.get('copilot-open')!({ payload: undefined }); });
    await flush();
    expect(tab(r, 'Copilot').props['aria-selected']).toBe(true);
    expect(localStorage.getItem('conclave.liveSideTab')).toBe('copilot');
    expect(localStorage.getItem('conclave.notesPaneCollapsed')).toBe('0');
  });
});
