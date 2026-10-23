import { afterAll, afterEach, beforeEach, describe, expect, mock, spyOn, test } from 'bun:test';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  config: { ...await import('../../src/contexts/ConfigContext') },
  sidebar: { ...await import('../../src/components/Sidebar/SidebarProvider') },
  ollama: { ...await import('../../src/contexts/OllamaDownloadContext') },
  builtIn: { ...await import('../../src/components/BuiltInModelManager') },
  select: { ...await import('../../src/components/ui/select') },
  popover: { ...await import('../../src/components/ui/popover') },
  command: { ...await import('../../src/components/ui/command') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('../../src/contexts/ConfigContext', () => originals.config);
  mock.module('../../src/components/Sidebar/SidebarProvider', () => originals.sidebar);
  mock.module('../../src/contexts/OllamaDownloadContext', () => originals.ollama);
  mock.module('../../src/components/BuiltInModelManager', () => originals.builtIn);
  mock.module('../../src/components/ui/select', () => originals.select);
  mock.module('../../src/components/ui/popover', () => originals.popover);
  mock.module('../../src/components/ui/command', () => originals.command);
});

const invoke = mock(async (_command: string): Promise<unknown> => undefined);
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('../../src/contexts/ConfigContext', () => ({ useConfig: () => undefined }));
mock.module('../../src/components/Sidebar/SidebarProvider', () => ({ useSidebar: () => ({ serverAddress: '' }) }));
mock.module('../../src/contexts/OllamaDownloadContext', () => ({
  useOllamaDownload: () => ({ isDownloading: () => false, getProgress: () => 0, downloadingModels: new Set() }),
}));
mock.module('../../src/components/BuiltInModelManager', () => ({ BuiltInModelManager: () => null }));
// Portal-based dropdowns cannot mount under react-test-renderer; the Save button is outside them.
const hidden = () => null;
mock.module('../../src/components/ui/select', () => ({ Select: hidden, SelectContent: hidden, SelectItem: hidden, SelectTrigger: hidden, SelectValue: hidden }));
mock.module('../../src/components/ui/popover', () => ({ Popover: hidden, PopoverContent: hidden, PopoverTrigger: hidden }));
mock.module('../../src/components/ui/command', () => ({ Command: hidden, CommandEmpty: hidden, CommandGroup: hidden, CommandInput: hidden, CommandItem: hidden, CommandList: hidden }));
const { ModelSettingsModal } = await import('../../src/components/ModelSettingsModal');

const SECRET = 'sk-test-secret-value';
let renderer: ReactTestRenderer | undefined;

function textOf(node: ReactTestInstance): string {
  return node.children.map((c) => (typeof c === 'string' ? c : textOf(c))).join('');
}
function saveButton(): ReactTestInstance {
  return renderer!.root.findAll((n) => n.type === 'button' && textOf(n).trim() === 'Save')[0];
}

let logs: unknown[][];
let spies: Array<{ mockRestore: () => void }> = [];
beforeEach(() => {
  logs = [];
  spies = (['log', 'error', 'warn', 'info'] as const).map((level) =>
    spyOn(console, level).mockImplementation((...args: unknown[]) => { logs.push(args); }));
});
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  spies.forEach((spy) => spy.mockRestore());
});

describe('saving model settings', () => {
  test('the API key is never written to the console', async () => {
    const modelConfig = { provider: 'claude', model: 'claude-sonnet', whisperModel: 'large-v3', apiKey: SECRET } as any;
    await act(async () => {
      renderer = create(
        <ModelSettingsModal modelConfig={modelConfig} setModelConfig={() => {}} onSave={() => {}} skipInitialFetch layout="dialog" />,
      );
    });
    await act(async () => { saveButton().props.onClick(); });
    expect(JSON.stringify(logs)).not.toContain(SECRET);
  });
});
