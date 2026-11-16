import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { ReactElement } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  event: { ...await import('@tauri-apps/api/event') },
  sonner: { ...await import('sonner') },
  switchUi: { ...await import('../../src/components/ui/switch') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/components/ui/switch', () => originals.switchUi);
});

type Status = { tokenLast4: string | null; checkOnStart: boolean; lastCheck: { at: string; result: string } | null; currentVersion: string };
type Backup = { kind: 'pre-upgrade' | 'manual'; createdAt: string; fromVersion: string | null; version: string; sizeBytes: number; path: string };

const SECRET = 'ghp_secretvalue1234';
let status: Status;
let backups: Backup[];
let checkResult: Record<string, unknown>;
let failing: Set<string>;
const calls: Array<[string, unknown]> = [];

const invoke = mock(async (command: string, args?: unknown): Promise<unknown> => {
  calls.push([command, args]);
  if (failing.has(command)) throw `${command} failed`;
  switch (command) {
    case 'updates_get_status': return structuredClone(status);
    case 'updates_set_token': status = { ...status, tokenLast4: (args as { token: string }).token.slice(-4) }; return null;
    case 'updates_remove_token': status = { ...status, tokenLast4: null }; return null;
    case 'updates_set_check_on_start': return null;
    case 'updates_check': return checkResult;
    case 'backup_list': return structuredClone(backups);
    case 'backup_now': return { kind: 'manual', createdAt: '2026-11-16T10:00:00Z', fromVersion: null, version: '1.0.4', sizeBytes: 48_000_000, path: 'x' };
    case 'backup_open_folder': return null;
  }
  throw new Error(`Unexpected command: ${command}`);
});
const emit = mock(async () => {});
const toast = { success: mock((..._a: unknown[]) => {}), error: mock((..._a: unknown[]) => {}) };

mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('@tauri-apps/api/event', () => ({ emit, listen: async () => () => {} }));
mock.module('sonner', () => ({ toast }));
mock.module('../../src/components/ui/switch', () => ({
  Switch: ({ checked, onCheckedChange, ...rest }: { checked?: boolean; onCheckedChange?: (on: boolean) => void; [key: string]: unknown }) => (
    <button type="button" role="switch" aria-checked={checked} onClick={() => onCheckedChange?.(!checked)} {...rest} />
  ),
}));

const { UpdatesSettings } = await import('../../src/components/UpdatesSettings');
const { TOKEN_HELP } = await import('../../src/lib/updates');

let renderer: ReactTestRenderer | null = null;
const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
async function mount(node: ReactElement = <UpdatesSettings />) {
  renderer?.unmount();
  await act(async () => { renderer = create(node); });
  await flush();
}
const textOf = (n: ReactTestInstance): string => n.children.map((c) => (typeof c === 'string' ? c : textOf(c))).join('');
const allText = () => textOf(renderer!.root);
const byButton = (t: string) => {
  const f = renderer!.root.findAll((n) => n.type === 'button' && textOf(n) === t);
  if (!f.length) throw new Error(`no button "${t}"`);
  return f[0];
};
const press = async (t: string) => { await act(async () => { await byButton(t).props.onClick(); }); await flush(); };
const input = () => renderer!.root.findAll((n) => n.type === 'input' && n.props.id === 'update-token');
const countOf = (c: string) => calls.filter(([n]) => n === c).length;

beforeEach(() => {
  status = { tokenLast4: null, checkOnStart: true, lastCheck: null, currentVersion: '1.0.4' };
  backups = [{ kind: 'pre-upgrade', createdAt: '2026-11-15T10:00:00Z', fromVersion: '1.0.4', version: '1.1.0', sizeBytes: 48_000_000, path: 'a' }];
  checkResult = { status: 'up_to_date', version: null, notes: null, message: null };
  failing = new Set();
  calls.length = 0;
  emit.mockClear(); toast.success.mockClear(); toast.error.mockClear();
});

async function type(value: string) {
  await act(async () => { input()[0].props.onChange({ target: { value } }); });
}

describe('UpdatesSettings', () => {
  test('saving a token sends it trimmed and never shows it', async () => {
    await mount();
    expect(allText()).toContain('Current version v1.0.4');
    await type(`  ${SECRET}  `);
    await press('Save');
    expect(calls.find(([c]) => c === 'updates_set_token')![1]).toEqual({ token: SECRET });
    expect(input().length).toBe(0);
    expect(allText()).toContain('Token saved ••••1234');
    expect(allText()).not.toContain('ghp_secretvalue');
    expect(JSON.stringify(toast.success.mock.calls) + JSON.stringify(toast.error.mock.calls)).not.toContain('ghp_secret');
  });

  test('failed save keeps the input and shows the error', async () => {
    failing.add('updates_set_token');
    await mount();
    await type(SECRET);
    await press('Save');
    expect(allText()).toContain('updates_set_token failed');
    expect(input()[0].props.value).toBe(SECRET);
  });

  test('Test reports Works', async () => {
    status.tokenLast4 = '1234';
    await mount();
    await press('Test');
    expect(allText()).toContain('Works');
  });

  test('Test shows the GitHub error', async () => {
    status.tokenLast4 = '1234';
    checkResult = { status: 'error', version: null, notes: null, message: 'Token rejected by GitHub: check it has read access to Conclave.' };
    await mount();
    await press('Test');
    expect(allText()).toContain('Token rejected by GitHub: check it has read access to Conclave.');
    expect(allText()).not.toContain('Works');
  });

  test('Remove clears the saved token', async () => {
    status.tokenLast4 = '1234';
    await mount();
    await press('Remove');
    expect(countOf('updates_remove_token')).toBe(1);
    expect(input().length).toBe(1);
    expect(allText()).not.toContain('Token saved');
  });

  test('toggle saves and reverts on failure', async () => {
    await mount();
    const sw = () => renderer!.root.find((n) => n.type === 'button' && n.props['aria-label'] === 'Check for updates on start');
    await act(async () => { await sw().props.onClick(); });
    await flush();
    expect(calls.find(([c]) => c === 'updates_set_check_on_start')![1]).toEqual({ enabled: false });
    expect(sw().props['aria-checked']).toBe(false);
    failing.add('updates_set_check_on_start');
    await act(async () => { await sw().props.onClick(); });
    await flush();
    expect(sw().props['aria-checked']).toBe(false);
    expect(toast.error.mock.calls[0][0]).toBe('Could not save the setting');
  });

  test('Check for updates shows an available update', async () => {
    checkResult = { status: 'available', version: '1.2.0', notes: null, message: null };
    await mount();
    await press('Check for updates');
    expect(allText()).toContain('Conclave v1.2.0 is available.');
    expect(emit).not.toHaveBeenCalled();
  });

  test('Check for updates error shows inline', async () => {
    checkResult = { status: 'error', version: null, notes: null, message: 'Network down' };
    await mount();
    await press('Check for updates');
    expect(allText()).toContain('Network down');
  });

  test('status line shows the last check, and a failed load says so', async () => {
    status.lastCheck = { at: '2026-11-16T09:00:00Z', result: 'up to date' };
    await mount();
    expect(allText()).toContain('up to date');
    failing.add('updates_get_status');
    await mount();
    expect(allText()).toContain('Could not load update settings');
  });

  test('help line is verbatim', async () => {
    await mount();
    expect(allText()).toContain(TOKEN_HELP);
  });

  test('Back up now success reloads the list', async () => {
    await mount();
    await press('Back up now');
    expect(toast.success.mock.calls[0][0]).toBe('Backed up (48 MB)');
    expect(countOf('backup_list')).toBe(2);
  });

  test('Back up now failure shows an error and keeps the list', async () => {
    failing.add('backup_now');
    await mount();
    await press('Back up now');
    expect(toast.success).not.toHaveBeenCalled();
    expect(toast.error.mock.calls[0][0]).toBe('Backup failed');
    expect(countOf('backup_list')).toBe(1);
    expect(allText()).toContain('Before upgrade v1.0.4 → v1.1.0');
  });

  test('backup rows show date, label and size; empty and error states', async () => {
    await mount();
    expect(allText()).toContain('Before upgrade v1.0.4 → v1.1.0');
    expect(allText()).toContain('48 MB');
    backups = [];
    await mount();
    expect(allText()).toContain('No backups yet');
    failing.add('backup_list');
    await mount();
    expect(allText()).toContain('Could not list backups');
  });

  test('Open backups folder invokes the command', async () => {
    await mount();
    await press('Open backups folder');
    expect(countOf('backup_open_folder')).toBe(1);
  });
});
