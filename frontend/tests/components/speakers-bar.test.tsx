import { afterAll, afterEach, describe, expect, mock, spyOn, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingSpeakers, Person, SpeakerView } from '../../src/types';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  popover: { ...await import('../../src/components/ui/popover') },
  command: { ...await import('../../src/components/ui/command') },
  config: { ...await import('../../src/contexts/ConfigContext') },
};
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('../../src/components/ui/popover', () => originals.popover);
  mock.module('../../src/components/ui/command', () => originals.command);
  mock.module('../../src/contexts/ConfigContext', () => originals.config);
});

const pass = ({ children }: { children?: ReactNode }) => <>{children}</>;
mock.module('../../src/components/ui/popover', () => ({
  Popover: pass, PopoverAnchor: pass, PopoverTrigger: pass, PopoverContent: pass,
}));
// Plain stand-ins: cmdk needs DOM measurement that the test renderer does not provide
mock.module('../../src/components/ui/command', () => ({
  Command: pass,
  CommandList: pass,
  CommandGroup: pass,
  CommandInput: (props: { placeholder?: string; value?: string; onValueChange?: (v: string) => void }) => <input {...props} />,
  CommandItem: ({ children, onSelect, disabled }: { children?: ReactNode; onSelect?: () => void; disabled?: boolean }) => (
    <button type="button" disabled={disabled} onClick={() => onSelect?.()}>{children}</button>
  ),
}));

type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();
let meetingReply: MeetingSpeakers = { job: null, speakers: [] };
const people: Person[] = [
  { id: 'p1', name: 'Сергей', is_self: false, meetings: 2, last_heard: null },
  { id: 'me', name: 'Sasha', is_self: true, meetings: 30, last_heard: null },
];
const invoke = mock(async (command: string): Promise<unknown> => {
  if (command === 'speakers_get_meeting') return meetingReply;
  if (command === 'people_list') return people;
  return undefined;
});
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));
mock.module('@tauri-apps/api/event', () => ({
  ...originals.event,
  listen: async (event: string, handler: Handler) => {
    handlers.set(event, handler);
    return () => handlers.delete(event);
  },
}));

let settings = { speakerIdActive: true, speakerSettingsLoaded: true };
mock.module('../../src/contexts/ConfigContext', () => ({
  useConfig: () => ({ betaFeatures: { importAndRetranscribe: true }, ...settings }),
}));

const { SpeakersBar } = await import('../../src/components/MeetingDetails/SpeakersBar');
const { TranscriptButtonGroup } = await import('../../src/components/MeetingDetails/TranscriptButtonGroup');
const { useMeetingSpeakers } = await import('../../src/hooks/useMeetingSpeakers');
const { SpeakerLineMenu } = await import('../../src/components/MeetingDetails/SpeakerLineMenu');

const SERGEY: SpeakerView = { idx: 1, name: 'Сергей', person_id: 'p1', is_self: false, seconds: 1080 };
const SASHA: SpeakerView = { idx: 0, name: 'Sasha', person_id: 'me', is_self: true, seconds: 720 };
const UNNAMED: SpeakerView = { idx: 2, name: 'Speaker 2', person_id: null, is_self: false, seconds: 60 };

// The real hook wired into the bar, the way TranscriptPanel wires it
let renderer: ReactTestRenderer | undefined;
function Bar() {
  const s = useMeetingSpeakers('m1', true);
  return <SpeakersBar data={s.data} busy={s.busy} percent={s.percent} onName={s.name} onIdentify={s.identify} />;
}
async function mount(element: ReactNode) {
  await act(async () => { renderer = create(<>{element}</>); });
}
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  invoke.mockClear();
  handlers.clear();
  meetingReply = { job: null, speakers: [] };
  settings = { speakerIdActive: true, speakerSettingsLoaded: true };
});

const chips = (): ReactTestInstance[] => renderer!.root.findAll((n) => n.props['data-chip'] !== undefined && typeof n.type === 'string');
const commandItem = (value: string): ReactTestInstance => renderer!.root.findAll((n) => n.props.value === value && typeof n.type === 'function' && n.props.onSelect)[0];
const nameInput = (): ReactTestInstance => renderer!.root.findAll((n) => n.props.placeholder === 'Name…' && typeof n.type === 'function')[0];
// Visible text of the rendered tree, so a sentence split across text nodes still matches
const textOf = (node: unknown): string => {
  if (node == null || typeof node === 'boolean') return '';
  if (typeof node === 'string') return node;
  if (Array.isArray(node)) return node.map(textOf).join('');
  return textOf((node as { children?: unknown }).children);
};
const callsOf = (command: string) => invoke.mock.calls.filter(([c]) => c === command);

describe('SpeakersBar', () => {
  test('chips are grouped and ordered with self first, then talk time', async () => {
    meetingReply = { job: { status: 'done', reason: null, percent: null }, speakers: [SERGEY, UNNAMED, SASHA] };
    await mount(<Bar />);
    expect(chips().map((c) => c.props['data-chip'])).toEqual(['Sasha', 'Сергей', 'Speaker 2']);
    expect(textOf(renderer!.toJSON())).toContain('18m');
  });

  test('picking a known person names the speaker with its person id', async () => {
    meetingReply = { job: { status: 'done', reason: null, percent: null }, speakers: [UNNAMED] };
    await mount(<Bar />);
    await act(async () => { commandItem('Сергей').props.onSelect(); });
    expect(invoke).toHaveBeenCalledWith('speakers_name', { meetingId: 'm1', idx: 2, personId: 'p1' });
  });

  test('"New person…" with a name invokes speakers_name with newName', async () => {
    meetingReply = { job: { status: 'done', reason: null, percent: null }, speakers: [UNNAMED] };
    await mount(<Bar />);
    await act(async () => { nameInput().props.onValueChange('Аня'); });
    await act(async () => { commandItem('New person…').props.onSelect(); });
    expect(invoke).toHaveBeenCalledWith('speakers_name', { meetingId: 'm1', idx: 2, newName: 'Аня' });
  });

  test('while busy the chips are disabled and the progress shows', async () => {
    meetingReply = { job: { status: 'running', reason: null, percent: 40 }, speakers: [SERGEY, SASHA] };
    await mount(<Bar />);
    expect(chips().every((c) => c.props.disabled === true)).toBe(true);
    expect(textOf(renderer!.toJSON())).toContain('Identifying speakers… 40%');
  });

  test('a failed analysis shows the reason and Retry runs identify', async () => {
    meetingReply = { job: { status: 'failed', reason: 'boom', percent: null }, speakers: [] };
    await mount(<Bar />);
    expect(textOf(renderer!.toJSON())).toContain('Speaker analysis failed: boom. Retry');
    const retry = renderer!.root.findAll((n) => n.type === 'button' && n.props.children === 'Retry')[0];
    await act(async () => { await retry.props.onClick(); });
    expect(callsOf('speakers_identify').length).toBe(1);
  });

  test('a skipped job without a recording shows the quiet note', async () => {
    meetingReply = { job: { status: 'skipped', reason: 'no_recording', percent: null }, speakers: [] };
    await mount(<Bar />);
    expect(textOf(renderer!.toJSON())).toContain("No recording saved, so speakers weren't identified");
  });

  test('renders nothing when there are no speakers and no job note', async () => {
    await mount(<Bar />);
    expect(renderer!.toJSON()).toBeNull();
  });
});

// A line of the transcript with its speaker menu, moving through the real hook
function LineMenu({ idx, disabled = false }: { idx: number | null; disabled?: boolean }) {
  const s = useMeetingSpeakers('m1', true);
  return (
    <SpeakerLineMenu
      idx={idx}
      speakers={s.data?.speakers ?? []}
      disabled={disabled}
      onMove={(next) => void s.moveLine('t1', next)}
    />
  );
}

describe('SpeakerLineMenu', () => {
  test('choosing Unknown invokes speakers_move_line with idx null', async () => {
    meetingReply = { job: { status: 'done', reason: null, percent: null }, speakers: [SERGEY, SASHA] };
    await mount(<LineMenu idx={1} />);
    const unknown = renderer!.root.findAll((n) => n.props.role === 'menuitemradio' && n.props.children === 'Unknown')[0];
    await act(async () => { await unknown.props.onClick(); });
    expect(invoke).toHaveBeenCalledWith('speakers_move_line', { transcriptId: 't1', idx: null });
  });

  test('while a job runs the label is plain text with no menu', async () => {
    meetingReply = { job: { status: 'running', reason: null, percent: 10 }, speakers: [SERGEY, SASHA] };
    await mount(<LineMenu idx={1} disabled />);
    expect(renderer!.root.findAll((n) => n.props.role === 'menuitemradio')).toHaveLength(0);
    expect(renderer!.root.findAll((n) => n.props['data-speaker-label'] === 'Сергей')).not.toHaveLength(0);
  });
});

describe('identify button', () => {
  const EXACT = 'Labels, including your manual fixes, will be replaced. Continue?';
  const group = (job: MeetingSpeakers['job'] = null) => {
    const onIdentifySpeakers = mock(async () => {});
    return {
      onIdentifySpeakers,
      element: (
        <TranscriptButtonGroup
          transcriptCount={3}
          onCopyTranscript={() => {}}
          onOpenMeetingFolder={async () => {}}
          meetingId="m1"
          meetingFolderPath="/recordings/m1"
          audioMissingReason={null}
          speakerJob={job}
          speakersBusy={false}
          onIdentifySpeakers={onIdentifySpeakers}
        />
      ),
    };
  };
  const identifyButton = () => renderer!.root.findAll((n) => n.type === 'button' && (n.props.title === 'Identify speakers' || n.props.title === 'Re-identify'))[0];

  test('hidden when speaker id is inactive', async () => {
    settings = { speakerIdActive: false, speakerSettingsLoaded: true };
    await mount(group().element);
    expect(identifyButton()).toBeUndefined();
  });

  test('shown as Identify speakers when active and not yet analysed', async () => {
    await mount(group(null).element);
    expect(identifyButton()).toBeDefined();
    expect(identifyButton().props.title).toBe('Identify speakers');
  });

  test('Re-identify asks first and does nothing on No', async () => {
    const confirm = spyOn(window, 'confirm').mockImplementation(() => false);
    try {
      const { element, onIdentifySpeakers } = group({ status: 'done', reason: null, percent: null });
      await mount(element);
      expect(identifyButton().props.title).toBe('Re-identify');
      await act(async () => { identifyButton().props.onClick(); });
      expect(confirm).toHaveBeenCalledWith(EXACT);
      expect(onIdentifySpeakers).not.toHaveBeenCalled();
    } finally {
      confirm.mockRestore();
    }
  });

  test('Re-identify runs the job on Yes', async () => {
    const confirm = spyOn(window, 'confirm').mockImplementation(() => true);
    try {
      const { element, onIdentifySpeakers } = group({ status: 'done', reason: null, percent: null });
      await mount(element);
      await act(async () => { identifyButton().props.onClick(); });
      expect(onIdentifySpeakers).toHaveBeenCalledTimes(1);
    } finally {
      confirm.mockRestore();
    }
  });
});
