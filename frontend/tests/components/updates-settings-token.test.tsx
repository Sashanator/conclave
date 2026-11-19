import { afterAll, afterEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const originals = { updates: { ...(await import('../../src/lib/updates')) }, sonner: { ...(await import('sonner')) } };
afterAll(() => {
  mock.module('../../src/lib/updates', () => originals.updates);
  mock.module('sonner', () => originals.sonner);
});

let setFails = false;
const status = { tokenLast4: null, checkOnStart: true, lastCheck: null, currentVersion: '1.0.0' };
mock.module('sonner', () => ({ toast: { error() {}, success() {} } }));
mock.module('../../src/lib/updates', () => ({
  ...originals.updates,
  updatesGetStatus: async () => status,
  backupList: async () => [],
  updatesSetToken: async () => { if (setFails) throw new Error('bad token'); },
}));
const { UpdatesSettings } = await import('../../src/components/UpdatesSettings');

let r: ReactTestRenderer;
afterEach(() => { act(() => r?.unmount()); });
const flush = () => act(async () => { await new Promise((x) => setTimeout(x, 0)); });
const text = (n: any): string => n.children.map((c: any) => (typeof c === 'string' ? c : text(c))).join('');
async function saveWith(token: string) {
  await act(async () => { r = create(<UpdatesSettings />); });
  await flush();
  await act(async () => { r.root.findByProps({ id: 'update-token' }).props.onChange({ target: { value: token } }); });
  await act(async () => { await r.root.findAll((n) => n.type === 'button' && text(n) === 'Save')[0].props.onClick(); });
  await flush();
}

test('a saved token shows Saved', async () => {
  setFails = false;
  await saveWith('ghp_x');
  expect(text(r.root)).toContain('Saved ✓');
});

test('a failed token save shows the error and no Saved', async () => {
  setFails = true;
  await saveWith('ghp_x');
  expect(text(r.root)).toContain('bad token');
  expect(text(r.root)).not.toContain('Saved ✓');
});
