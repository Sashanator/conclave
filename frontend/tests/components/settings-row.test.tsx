import { afterEach, beforeEach, expect, test } from 'bun:test';
import { act, create } from 'react-test-renderer';
import { SettingsRow, useSavedTick } from '../../src/components/Settings/SettingsRow';

const text = (r: ReturnType<typeof create>) => JSON.stringify(r.toJSON());

test('label is a <label for> when htmlFor is given, text otherwise', () => {
  const a = create(<SettingsRow label="Theme" htmlFor="t" />);
  expect(a.root.findByType('label').props.htmlFor).toBe('t');
  const b = create(<SettingsRow label="Theme" />);
  expect(b.root.findAllByType('label')).toHaveLength(0);
  expect(text(b)).toContain('Theme');
});

test('helper renders', () => expect(text(create(<SettingsRow label="Theme" helper="Pick one" />))).toContain('Pick one'));

test('saved shows Saved inside role=status; absent otherwise', () => {
  const on = create(<SettingsRow label="Theme" saved />);
  expect(JSON.stringify(on.root.findByProps({ role: 'status' }).children)).toContain('Saved ✓');
  expect(text(create(<SettingsRow label="Theme" />))).not.toContain('Saved');
});

test('row id comes from the label', () => {
  const r = create(<SettingsRow label="Windows notifications when recording starts/stops" />);
  expect(r.root.findAllByProps({ id: 'setting-windows-notifications-when-recording-starts-stops' }).length).toBeGreaterThan(0);
});

let realSet = globalThis.setTimeout;
beforeEach(() => { realSet = globalThis.setTimeout; });
afterEach(() => { globalThis.setTimeout = realSet; });

test('useSavedTick: true after markSaved, false after 2 s, restarts on a new call', async () => {
  let api!: ReturnType<typeof useSavedTick>;
  const H = () => { api = useSavedTick(); return null; };
  act(() => { create(<H />); });
  act(() => api[1]());
  expect(api[0]).toBe(true);
  await act(async () => { await new Promise((r) => realSet(r, 1200)); });
  act(() => api[1]());
  await act(async () => { await new Promise((r) => realSet(r, 1200)); });
  expect(api[0]).toBe(true); // restarted: 2.4 s since the first call, still shown
  await act(async () => { await new Promise((r) => realSet(r, 1000)); });
  expect(api[0]).toBe(false);
});
