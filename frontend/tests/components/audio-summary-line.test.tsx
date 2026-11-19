import { afterAll, expect, mock, test } from 'bun:test';
import { cloneElement, createContext, useContext, useState, type ReactElement, type ReactNode } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  config: { ...await import('../../src/contexts/ConfigContext') },
  device: { ...await import('../../src/components/DeviceSelection') },
  language: { ...await import('../../src/components/LanguageSelection') },
  popover: { ...await import('../../src/components/ui/popover') },
  recording: { ...await import('../../src/contexts/RecordingStateContext') },
};
afterAll(() => {
  mock.module('../../src/contexts/ConfigContext', () => originals.config);
  mock.module('../../src/components/DeviceSelection', () => originals.device);
  mock.module('../../src/components/LanguageSelection', () => originals.language);
  mock.module('../../src/components/ui/popover', () => originals.popover);
  mock.module('../../src/contexts/RecordingStateContext', () => originals.recording);
});

mock.module('../../src/contexts/ConfigContext', () => ({
  useConfig: () => ({
    selectedDevices: { micDevice: 'Headset (input)', systemDevice: 'Speakers (output)' }, setSelectedDevices() {},
    selectedLanguage: 'auto', setSelectedLanguage() {}, allowedLanguages: ['ru', 'en', 'de'], transcriptModelConfig: { provider: 'localWhisper' },
  }),
}));
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals.recording, useRecordingState: () => ({ micFollowing: { warning: null } }) }));
mock.module('../../src/components/DeviceSelection', () => ({ DeviceSelection: () => <span>DEVICE PICKER</span> }));
mock.module('../../src/components/LanguageSelection', () => ({ LanguageSelection: () => <span>Language</span> }));
const Open = createContext<[boolean, (o: boolean) => void]>([false, () => {}]);
mock.module('../../src/components/ui/popover', () => ({
  Popover: ({ children }: { children?: ReactNode }) => <Open.Provider value={useState(false)}>{children}</Open.Provider>,
  PopoverTrigger: ({ children }: { children: ReactElement }) => { const [, set] = useContext(Open); return cloneElement(children, { onClick: () => set(true) }); },
  PopoverContent: ({ children }: { children?: ReactNode }) => (useContext(Open)[0] ? <>{children}</> : null),
}));

const { AudioSummaryLine } = await import('../../src/components/MeetingPage/AudioSummaryLine');
const text = (r: ReactTestRenderer) => JSON.stringify(r.toJSON());
const mount = async (changeable: boolean) => { let r!: ReactTestRenderer; await act(async () => { r = create(<AudioSummaryLine changeable={changeable} />); }); return r; };

test('shows the summary; Change opens the pickers', async () => {
  const r = await mount(true);
  expect(text(r)).toContain('Mic: Headset · System audio: Speakers · Language: Auto (ru, en, de)');
  expect(text(r)).not.toContain('DEVICE PICKER');
  await act(async () => { r.root.findByType('button').props.onClick(); });
  expect(text(r)).toContain('DEVICE PICKER');
});

test('not changeable: no Change button', async () => {
  const r = await mount(false);
  expect(text(r)).toContain('Mic: Headset');
  expect(r.root.findAllByType('button').length).toBe(0);
});
