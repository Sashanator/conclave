import { afterAll, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = { config: { ...await import('../../src/contexts/ConfigContext') } };
afterAll(() => {
  mock.module('../../src/contexts/ConfigContext', () => originals.config);
});
mock.module('../../src/contexts/ConfigContext', () => ({
  ...originals.config,
  useConfig: () => ({ betaFeatures: { importAndRetranscribe: false }, speakerIdActive: false }),
}));

const { TranscriptButtonGroup } = await import('../../src/components/MeetingDetails/TranscriptButtonGroup');

const textOf = (n: ReactTestInstance): string => n.children.map((c) => (typeof c === 'string' ? c : textOf(c))).join('');

function render(extra: Record<string, unknown> = {}): ReactTestRenderer {
  let r!: ReactTestRenderer;
  act(() => {
    r = create(
      <TranscriptButtonGroup
        transcriptCount={3}
        onCopyTranscript={() => {}}
        onOpenMeetingFolder={async () => {}}
        {...extra}
      />,
    );
  });
  return r;
}

describe('Resume recording button', () => {
  test('Resume recording shows only with a handler and calls it', async () => {
    const without = render();
    expect(without.root.findAll((n) => n.type === 'button' && textOf(n).includes('Resume recording')).length).toBe(0);

    let called = 0;
    const withHandler = render({ onResumeRecording: () => { called++; } });
    const button = withHandler.root.findAll((n) => n.type === 'button' && textOf(n).includes('Resume recording'));
    expect(button.length).toBe(1);
    await act(async () => { await button[0].props.onClick(); });
    expect(called).toBe(1);
  });
});
