import { afterAll, describe, expect, mock, spyOn, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const originalCore = { ...await import('@tauri-apps/api/core') };
const originalEvent = { ...await import('@tauri-apps/api/event') };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originalCore);
  mock.module('@tauri-apps/api/event', () => originalEvent);
});

// Only speakers_get_settings matters here; every other command answers with an empty list
let settingsReply: () => Promise<unknown> = async () => { throw new Error('Command speakers_get_settings not found'); };
const invoke = mock(async (command: string): Promise<unknown> => {
  if (command === 'speakers_get_settings') return settingsReply();
  return [];
});
mock.module('@tauri-apps/api/core', () => ({ ...originalCore, invoke }));

const handlers = new Map<string, () => void>();
mock.module('@tauri-apps/api/event', () => ({
  ...originalEvent,
  listen: async (event: string, handler: () => void) => {
    handlers.set(event, handler);
    return () => handlers.delete(event);
  },
}));

const { ConfigProvider, useConfig } = await import('../../src/contexts/ConfigContext');

let latest: ReturnType<typeof useConfig>;
function Probe() {
  latest = useConfig();
  return null;
}

async function mount(): Promise<ReactTestRenderer> {
  let renderer!: ReactTestRenderer;
  await act(async () => {
    renderer = create(<ConfigProvider><Probe /></ConfigProvider>);
  });
  return renderer;
}

describe('speaker settings in ConfigContext', () => {
  test('is not loaded until the first lookup answers', async () => {
    settingsReply = () => new Promise(() => {});
    await mount();
    expect(latest.speakerSettingsLoaded).toBe(false);
    expect(latest.speakerIdActive).toBe(false);
  });

  test('a failed lookup leaves speaker id off but marks settings loaded', async () => {
    settingsReply = async () => { throw new Error('Command speakers_get_settings not found'); };
    const logged = spyOn(console, 'error').mockImplementation(() => {});
    try {
      await mount();
    } finally {
      logged.mockRestore();
    }
    expect(latest.speakerSettingsLoaded).toBe(true);
    expect(latest.speakerIdActive).toBe(false);
  });

  test('active only when enabled and the models are ready', async () => {
    settingsReply = async () => ({ enabled: true, models_ready: false, downloading: false, last_analysis: null });
    await mount();
    expect(latest.speakerIdActive).toBe(false);

    settingsReply = async () => ({ enabled: true, models_ready: true, downloading: false, last_analysis: null });
    await mount();
    expect(latest.speakerIdActive).toBe(true);
  });

  test('speaker-settings-changed re-reads the settings', async () => {
    settingsReply = async () => ({ enabled: true, models_ready: true, downloading: false, last_analysis: null });
    await mount();
    expect(latest.speakerIdActive).toBe(true);

    settingsReply = async () => ({ enabled: false, models_ready: true, downloading: false, last_analysis: null });
    await act(async () => {
      handlers.get('speaker-settings-changed')!();
    });
    expect(latest.speakerIdActive).toBe(false);
  });
});
